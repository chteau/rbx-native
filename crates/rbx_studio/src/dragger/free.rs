//! A free (body) drag: where the dragged point lands on the face under the
//! cursor, and what Studio draws while it does (`DragHelper.getDragTargetNew`,
//! `FreeformDragger:_renderSnap`).
//!
//! Studio moves a free drag by one point on the selection, the *dragged
//! point*, grabbed at the press. Every frame the point's foot — the point
//! dropped square onto the face under the cursor — goes to where the cursor
//! meets that face, corrected onto the face's grid and, with Snap to Parts,
//! onto the face's edges and centre lines. The selection itself rests on the
//! face, its box's underside flush with it.

use glam::{Mat3, Mat4, Vec3};
use rbx_viewer::Pose;

use super::surface::{surface_frame, SurfaceFrame, TargetKind};
use super::{handle_scale, round, sign, snap_to, Dot, Guides, Line, ACTIVE, SOFT_SNAP_MARGIN};

/// How near an alignment must be to pull a free drag onto it, in units of
/// [`super::depth_scale`] (`MULK … [0.02]`): about 14.5 pixels at 1080p with a
/// 70° field of view.
pub(crate) const SOFT_SNAP_REACH: f32 = 0.02;

/// The dragged point's dot, in handle scales; its bar is [`BAR_WIDTH`] wide.
const DOT_RADIUS: f32 = 0.15;
const BAR_WIDTH: f32 = 0.05;
/// How far an alignment line runs on past each end, in handle scales.
const LINE_OVERRUN: f32 = 1.5;
/// An alignment line with no length — every one on a face with no size, a
/// ball's pole — is a mark this wide instead. Studio draws a cube
/// (`BoxHandleAdornment`) there; the line layer draws the same-sized dot.
const POINT_SIZE: f32 = 0.3;

/// The grab at a press, snapped the way Studio snaps it
/// (`dispatchWorldClick`): the point clicked on `model`, rounded onto the
/// grid of the frame the hover under the click stood on (`hovered`, or the
/// clicked box face's own when there was none) across the face — its height
/// off the face kept — or, on a ball, onto the grid of the ball's own frame
/// on every axis. Snapped whenever the toolbar's snapping is on; `Shift`
/// does not change it.
pub(crate) fn grab(
    model: Mat4,
    point: Vec3,
    grid: f32,
    ball: bool,
    hovered: Option<SurfaceFrame>,
) -> Vec3 {
    if grid <= 0.0 {
        return point;
    }
    if ball {
        let inverse = model.inverse();
        let scale = Vec3::new(
            model.x_axis.truncate().length(),
            model.y_axis.truncate().length(),
            model.z_axis.truncate().length(),
        );
        let local = inverse.transform_point3(point) * scale;
        let snapped = Vec3::new(
            snap_to(local.x, grid),
            snap_to(local.y, grid),
            snap_to(local.z, grid),
        );
        return model.transform_point3(snapped / scale);
    }
    let Some(frame) = hovered.or_else(|| surface_frame(model, point)) else {
        return point;
    };
    let local = frame.local(point);
    frame.world(Vec3::new(
        snap_to(local.x, grid),
        local.y,
        snap_to(local.z, grid),
    ))
}

/// Studio's selection box (`getLocalBoundingBox`): the tight box round the
/// parts' own boxes `models`, in the frame of the selection's basis
/// (`orientation` at `origin`) — its centre in that frame, and its size.
pub(crate) fn selection_box(
    orientation: Mat3,
    origin: Vec3,
    models: impl IntoIterator<Item = Mat4>,
) -> (Vec3, Vec3) {
    let inverse = orientation.transpose();
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for model in models {
        let centre = inverse * (model.w_axis.truncate() - origin);
        let reach = (inverse * Mat3::from_mat4(model)).abs() * Vec3::splat(0.5);
        low = low.min(centre - reach);
        high = high.max(centre + reach);
    }
    ((low + high) * 0.5, high - low)
}

/// The selection box (`centre` and `size` in the basis frame) turned by
/// `turn` into a target frame's axes and measured from the dragged point
/// (`dragged`, in the basis frame): its low and high corners there — the
/// box Studio lands (`getSizeInSpace`, round `(rot·tilt)(offset − point)`).
pub(crate) fn bounds(turn: Mat3, (centre, size): (Vec3, Vec3), dragged: Vec3) -> (Vec3, Vec3) {
    let middle = turn * (centre - dragged);
    let half = turn.abs() * size * 0.5;
    (middle - half, middle + half)
}

/// Where a free drag lands for this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Landing {
    /// The dragged point's foot on the face: the cursor, snapped.
    pub(crate) foot: Vec3,
    /// How far above the face the dragged point stands once the selection's
    /// box rests on it.
    pub(crate) lift: f32,
    /// The face's edge and centre lines the landing was pulled onto, one per
    /// axis Studio chose a soft snap for, each spanning the whole face.
    pub(crate) aligned: Vec<[Vec3; 2]>,
}

impl Landing {
    /// Where the dragged point itself ends up.
    pub(crate) fn dragged(&self, frame: &SurfaceFrame) -> Vec3 {
        self.foot + frame.y * self.lift
    }
}

/// Lands the dragged point for a cursor meeting `frame`'s face at `hit`, with
/// the selection's box `bounds` (see [`bounds`]) round it. `grid` is the grid
/// in force (`0.0` for none: off, or `Shift` held); `reach` is how near a
/// face alignment has to be to pull the landing onto it, or `None` with Snap
/// to Parts off.
///
/// Per axis of the face, the grid and the alignment each propose a
/// correction; Studio takes the grid's unless the alignment's is the smaller
/// by more than a hundredth of a stud.
///
/// Only a flat face (a [`TargetKind::Polygon`]) aligns. A ball's frame
/// stands on its snapped point already, and the foot goes there exactly;
/// every other kind takes the grid alone.
pub(crate) fn land(
    frame: &SurfaceFrame,
    hit: Vec3,
    bounds: (Vec3, Vec3),
    grid: f32,
    reach: Option<f32>,
) -> Landing {
    let local = match frame.kind {
        TargetKind::Sphere => Vec3::ZERO,
        _ => frame.local(hit),
    };
    let reach = reach.filter(|_| frame.kind == TargetKind::Polygon);
    let inside = Vec3::new(sign(local.x), 0.0, sign(local.z));
    let (low, high) = bounds;
    let centre = (low + high) * 0.5;
    let half = (high - low) * 0.5;

    let mut aligned = Vec::new();
    let mut corrected = [0.0f32; 2];
    for (slot, axis) in [(0usize, 0usize), (1, 2)] {
        let at = local[axis];
        let snapped = (grid > 0.0).then(|| snap_to(at, grid) - at);
        let size = [frame.size.x, frame.size.y][slot];
        let soft = reach.and_then(|reach| {
            let (miss, line) = align(at + centre[axis], half[axis], inside[axis], size);
            (miss.abs() < reach * SOFT_SNAP_MARGIN).then_some((-miss, line))
        });
        let (correction, line) = match (snapped, soft) {
            (Some(grid), Some((soft, line))) => {
                if (grid.abs() - soft.abs()).abs() < 0.01 || grid.abs() < soft.abs() {
                    (grid, None)
                } else {
                    (soft, Some(line))
                }
            }
            (Some(grid), None) => (grid, None),
            (None, Some((soft, line))) => (soft, Some(line)),
            (None, None) => (0.0, None),
        };
        corrected[slot] = correction;
        if let Some(line) = line {
            // Across the whole face, from the corner's edge to the far one.
            let other = if axis == 0 { 2 } else { 0 };
            let span = [frame.size.x, frame.size.y][1 - slot] * inside[other];
            let mut from = Vec3::ZERO;
            from[axis] = line;
            let mut to = from;
            to[other] = span;
            aligned.push([frame.world(from), frame.world(to)]);
        }
    }

    let foot = frame.world(Vec3::new(
        local.x + corrected[0],
        0.0,
        local.z + corrected[1],
    ));
    Landing {
        foot,
        lift: -low.y,
        aligned,
    }
}

/// The nearest of the nine alignments between a dragged box centred at
/// `centre` and `half` wide and a face `size` long whose inside lies towards
/// `inside` from the corner: the box's low side, middle or high side against
/// the face's near edge, centre line or far edge. How far it misses, and
/// where the face's line is.
fn align(centre: f32, half: f32, inside: f32, size: f32) -> (f32, f32) {
    let middle = 0.5 * inside * size;
    let mut best = (f32::INFINITY, 0.0);
    for box_side in [-1.0, 0.0, 1.0] {
        for face_side in [-1.0, 0.0, 1.0] {
            let line = middle + face_side * 0.5 * size;
            let miss = centre + box_side * half - line;
            if miss.abs() < best.0.abs() {
                best = (miss, line);
            }
        }
    }
    best
}

/// What Studio draws for a free drag landing on `frame`'s face: the face
/// alignments it snapped to, if any, or else the ruler to the landing point
/// — on a ball or a cylinder, their own guides in its place (see
/// [`round::landed`]); and, with the grid snapping, the dragged point's dot
/// and its bar down to the face.
///
/// `target_snap` and `dragged_point` are the Show Target Snap and Show
/// Dragged Point settings.
#[allow(clippy::too_many_arguments)]
pub(crate) fn guides(
    frame: &SurfaceFrame,
    hit: Vec3,
    landing: &Landing,
    grid: f32,
    target_snap: bool,
    dragged_point: bool,
    pose: Pose,
    orthographic: bool,
) -> Guides {
    let scale = |point: Vec3| handle_scale(point, pose, orthographic);
    let mut guides = Guides::default();
    let soft = !landing.aligned.is_empty();
    if target_snap {
        match round::landed(frame, hit, grid, soft, scale) {
            Some(round) => guides = round,
            None if !soft => guides.lines = super::ruler::target(frame, hit, grid),
            None => {}
        }
    }
    if target_snap && soft {
        for &[from, to] in &landing.aligned {
            let scale = scale((from + to) * 0.5);
            let Some(direction) = (to - from).try_normalize() else {
                guides.dots.push(Dot {
                    centre: from,
                    radius: 0.5 * POINT_SIZE * scale,
                    color: ACTIVE,
                });
                continue;
            };
            let overrun = direction * scale * LINE_OVERRUN;
            guides.lines.push(Line::hairline(
                from - overrun,
                to + overrun,
                ACTIVE,
                1.0,
                0.4,
            ));
        }
    }
    // Drawn beside the alignment lines too. The decompiled `SnapConnection`
    // reads as skipping it while anything is soft-snapped, but Studio's own
    // screen shows the dot and its bar with the alignment lines, and the
    // screen is what the draggers actually do.
    if dragged_point && grid > 0.0 {
        let point = landing.dragged(frame);
        let scale = scale(point);
        guides.dots.push(Dot {
            centre: point,
            radius: DOT_RADIUS * scale,
            color: ACTIVE,
        });
        guides.lines.push(Line {
            from: point,
            to: frame.onto_plane(point),
            color: ACTIVE,
            under: 0.0,
            over: 1.0,
            width: BAR_WIDTH * scale,
        });
    }
    guides
}

#[cfg(test)]
#[path = "free/tests.rs"]
mod tests;
