//! Studio's `RulerView`, and the two guides built on it: the white hover
//! ruler (`HoverSnapDisplay`) and the yellow ruler a free drag lands along
//! (`TargetGridView`).
//!
//! A ruler is drawn from a face's nearest corner to a grid-snapped point `P`
//! on the face: two lines, each dropping from `P` square onto one of the two
//! edges that meet at the corner, ticked at every grid step. The ticks are
//! world lengths that scale with the grid, not with the screen.

use glam::Vec3;
use rbx_viewer::Pose;

use super::surface::SurfaceFrame;
use super::{handle_scale, snap_to, Dot, Guides, Line, ACTIVE, MAJOR_GRID_INCREMENT, PASSIVE};

/// Half a minor tick's length, in grid steps (`MULK … [0.15]`).
const MINOR_TICK: f32 = 0.15;
/// Half a major tick's length, in grid steps (`MULK … [0.65]`).
const MAJOR_TICK: f32 = 0.65;
/// A line that would need this many ticks or more gets none, and the edge
/// segment it measures along is drawn instead.
const MAX_TICKS: i32 = 48;

/// The hover dot's radius, in handle scales.
const DOT_RADIUS: f32 = 0.15;

/// The segments of a ruler whose corner is `origin`, measuring `size` along
/// `x` and `z` (both pointing into the face) with ticks every `grid` studs.
fn ruler(origin: Vec3, x: Vec3, z: Vec3, size: (f32, f32), grid: f32) -> Vec<[Vec3; 2]> {
    let at = |a: f32, b: f32| origin + x * a + z * b;
    let (sx, sz) = size;
    let mut lines = vec![[at(sx, 0.0), at(sx, sz)], [at(0.0, sz), at(sx, sz)]];

    let tick = |i: i32| {
        let major = i % MAJOR_GRID_INCREMENT as i32 == 0;
        grid * if major { MAJOR_TICK } else { MINOR_TICK }
    };
    // Ticks along the line z = sz, counted from the x = 0 edge: the first is
    // the long cap on the edge itself, the last falls on P.
    let count = (sx / grid + 0.001).floor() as i32 + 1;
    if count < MAX_TICKS {
        for i in 0..count {
            let (along, half) = (i as f32 * grid, tick(i));
            lines.push([at(along, sz - half), at(along, sz + half)]);
        }
    } else {
        lines.push([at(0.0, 0.0), at(sx, 0.0)]);
    }
    let count = (sz / grid + 0.001).floor() as i32 + 1;
    if count < MAX_TICKS {
        for i in 0..count {
            let (along, half) = (i as f32 * grid, tick(i));
            lines.push([at(sx - half, along), at(sx + half, along)]);
        }
    } else {
        lines.push([at(0.0, 0.0), at(0.0, sz)]);
    }
    lines
}

/// The frame's in-plane axes, each flipped towards `local` — the quadrant of
/// the face the point is in, seen from the corner.
fn towards(frame: &SurfaceFrame, local: Vec3) -> (Vec3, Vec3) {
    let x = if 0.0 < local.x { frame.x } else { -frame.x };
    let z = if 0.0 < local.z { frame.z } else { -frame.z };
    (x, z)
}

/// Studio's hover ruler over `frame`'s face with the cursor on `hit`: white,
/// to the grid point nearest the cursor, with a dot on that point while the
/// grid actually snaps (`grid_snap`: snapping on and `Shift` up). `pending`
/// is a press on a part that has not become a drag yet, which turns the dot
/// yellow.
///
/// The caller decides whether a hover ruler shows at all: Studio needs the
/// toolbar's snapping switched on (`Shift` does not hide it), the setting on,
/// and no handle under the cursor.
pub(crate) fn hover(
    frame: &SurfaceFrame,
    hit: Vec3,
    grid: f32,
    grid_snap: bool,
    pending: bool,
    pose: Pose,
    orthographic: bool,
) -> Guides {
    if grid <= 0.0 {
        return Guides::default();
    }
    let local = frame.local(hit);
    let (x, z) = towards(frame, local);
    let snapped = Vec3::new(snap_to(local.x, grid), 0.0, snap_to(local.z, grid));
    let lines = ruler(frame.corner, x, z, (snapped.x.abs(), snapped.z.abs()), grid)
        .into_iter()
        .map(|[from, to]| Line::hairline(from, to, PASSIVE, 0.6, 0.15))
        .collect();

    let point = frame.world(snapped);
    let dots = grid_snap
        .then(|| Dot {
            centre: point,
            radius: DOT_RADIUS * handle_scale(point, pose, orthographic),
            color: if pending { ACTIVE } else { PASSIVE },
        })
        .into_iter()
        .collect();
    Guides { lines, dots }
}

/// The yellow ruler a free drag with the grid snapping shows on the face it
/// is landing on, from the face's corner to where the dragged point lands
/// (`hit`, the cursor on the face, rounded onto the grid).
pub(crate) fn target(frame: &SurfaceFrame, hit: Vec3, grid: f32) -> Vec<Line> {
    if grid <= 0.0 {
        return Vec::new();
    }
    let relative = hit - frame.corner;
    let (x, z) = towards(frame, frame.local(hit));
    let size = |along: Vec3| (relative.dot(along).max(0.0) / grid).round() * grid;
    ruler(frame.corner, x, z, (size(x), size(z)), grid)
        .into_iter()
        .map(|[from, to]| Line::hairline(from, to, ACTIVE, 1.0, 0.5))
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec2};

    use super::super::surface::surface_frame;
    use super::*;

    fn pose() -> Pose {
        Pose {
            position: Vec3::new(0.0, 20.0, 20.0),
            yaw: 0.0,
            pitch: -0.7,
            fov_degrees: 70.0,
            ortho_scale: 10.0,
        }
    }

    /// The top face of a 20 × 1 × 20 plate, cornered on (10, 0.5, 10).
    fn top() -> SurfaceFrame {
        surface_frame(
            Mat4::from_scale(Vec3::new(20.0, 1.0, 20.0)),
            Vec3::new(7.0, 0.5, 8.0),
        )
        .unwrap()
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_ruler_drops_from_the_point_square_onto_both_edges() {
        let lines = ruler(Vec3::ZERO, Vec3::X, Vec3::Z, (3.0, 2.0), 1.0);
        assert_eq!(
            lines[0],
            [Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 2.0)]
        );
        assert_eq!(
            lines[1],
            [Vec3::new(0.0, 0.0, 2.0), Vec3::new(3.0, 0.0, 2.0)]
        );
        // Four ticks along the first (0, 1, 2, 3) and three along the second.
        assert_eq!(lines.len(), 2 + 4 + 3);
    }

    #[test]
    fn the_tick_on_the_edge_and_every_fifth_after_it_are_long() {
        let lines = ruler(Vec3::ZERO, Vec3::X, Vec3::Z, (7.0, 0.0), 2.0);
        let ticks: Vec<f32> = lines[2..6].iter().map(|[a, b]| (b - a).length()).collect();
        // Ticks at 0, 2, 4, 6: the first is a cap, the rest short.
        for (tick, expected) in ticks.into_iter().zip([2.6, 0.6, 0.6, 0.6]) {
            assert!((tick - expected).abs() < 1e-5, "{tick} vs {expected}");
        }
        let long = ruler(Vec3::ZERO, Vec3::X, Vec3::Z, (5.0, 0.0), 1.0);
        let half = |i: usize| (long[2 + i][1] - long[2 + i][0]).length() / 2.0;
        assert!((half(0) - 0.65).abs() < 1e-6 && (half(5) - 0.65).abs() < 1e-6);
        assert!((half(1) - 0.15).abs() < 1e-6);
    }

    #[test]
    fn forty_eight_ticks_or_more_become_the_bare_edge() {
        // 47 steps is 48 ticks: none are drawn, the edge segment is.
        let lines = ruler(Vec3::ZERO, Vec3::X, Vec3::Z, (47.0, 1.0), 1.0);
        assert_eq!(lines[2], [Vec3::ZERO, Vec3::new(47.0, 0.0, 0.0)]);
        assert_eq!(lines.len(), 2 + 1 + 2);
        let lines = ruler(Vec3::ZERO, Vec3::X, Vec3::Z, (46.0, 1.0), 1.0);
        assert_eq!(lines.len(), 2 + 47 + 2);
    }

    #[test]
    fn the_hover_dot_sits_on_the_grid_point_nearest_the_cursor() {
        let frame = top();
        let guides = hover(
            &frame,
            Vec3::new(7.3, 0.5, 8.6),
            1.0,
            true,
            false,
            pose(),
            false,
        );
        assert_eq!(guides.dots.len(), 1);
        // 2.7 and 1.4 in from the corner round to 3 and 1.
        assert!(close(guides.dots[0].centre, Vec3::new(7.0, 0.5, 9.0)));
        assert_eq!(guides.dots[0].color, PASSIVE);
        assert!(guides
            .lines
            .iter()
            .all(|line| line.color == PASSIVE && line.under == 0.6 && line.over == 0.15));
    }

    #[test]
    fn shift_hides_the_dot_but_not_the_ruler_and_a_press_turns_it_yellow() {
        let frame = top();
        let hit = Vec3::new(7.3, 0.5, 8.6);
        let shifted = hover(&frame, hit, 1.0, false, false, pose(), false);
        assert!(shifted.dots.is_empty() && !shifted.lines.is_empty());
        let pressed = hover(&frame, hit, 1.0, true, true, pose(), false);
        assert_eq!(pressed.dots[0].color, ACTIVE);
    }

    #[test]
    fn the_hover_ruler_runs_into_the_face_whichever_way_the_frame_points() {
        let frame = top();
        let guides = hover(
            &frame,
            Vec3::new(7.3, 0.5, 8.6),
            1.0,
            true,
            false,
            pose(),
            false,
        );
        for line in &guides.lines {
            for end in [line.from, line.to] {
                assert!(end.x <= 10.0 + 0.66 && end.z <= 10.0 + 0.66, "{end}");
            }
        }
    }

    #[test]
    fn the_drag_ruler_ends_on_the_rounded_landing_point() {
        let frame = top();
        let lines = target(&frame, Vec3::new(6.6, 0.5, 7.2), 1.0);
        // 3.4 and 2.8 in round to 3 and 3: P at (7, 0.5, 7).
        assert!(lines.iter().all(|line| line.color == ACTIVE));
        let corner_to_p = &lines[..2];
        assert!(corner_to_p
            .iter()
            .all(|line| close(line.to, Vec3::new(7.0, 0.5, 7.0))));
        assert_eq!(frame.size, Vec2::new(20.0, 20.0));
    }

    #[test]
    fn no_grid_no_ruler() {
        assert!(target(&top(), Vec3::new(6.6, 0.5, 7.2), 0.0).is_empty());
        assert_eq!(
            hover(
                &top(),
                Vec3::new(6.6, 0.5, 7.2),
                0.0,
                false,
                false,
                pose(),
                false
            ),
            Guides::default()
        );
    }
}
