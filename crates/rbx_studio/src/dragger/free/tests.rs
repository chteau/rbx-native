use glam::{Mat3, Mat4, Vec2, Vec3};
use rbx_viewer::pick::Solid;

use super::*;
use crate::dragger::PASSIVE;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn pose() -> Pose {
    Pose {
        position: Vec3::new(0.0, 20.0, 30.0),
        yaw: 0.0,
        pitch: -0.6,
        fov_degrees: 70.0,
        ortho_scale: 10.0,
    }
}

/// The top face of a 20 × 1 × 20 plate centred on the origin, framed for a
/// cursor near its (10, 0.5, 10) corner.
fn plate() -> SurfaceFrame {
    surface_frame(
        Mat4::from_scale(Vec3::new(20.0, 1.0, 20.0)),
        Vec3::new(7.0, 0.5, 8.0),
    )
    .unwrap()
}

/// A box `size` big standing square to the world, grabbed by the middle of
/// its top face: its bounds measured from that point in `frame`, as it is
/// held (not turned onto the face).
fn grabbed_by_top(frame: &SurfaceFrame, size: Vec3) -> (Vec3, Vec3) {
    let turn = crate::dragger::tilt::frame_rotation(frame).transpose();
    bounds(turn, (Vec3::ZERO, size), Vec3::new(0.0, size.y * 0.5, 0.0))
}

#[test]
fn a_press_snaps_the_grab_onto_the_clicked_faces_own_grid() {
    // A 4 × 1 × 2 block; its top face's corner nearest (0.3, 0.5, 0.4) is
    // (2, 0.5, 1), so the whole-stud points of that face are x ∈ {-2…2},
    // z ∈ {-1, 0, 1}.
    let block = Mat4::from_scale(Vec3::new(4.0, 1.0, 2.0));
    let grabbed = grab(block, Vec3::new(0.3, 0.5, 0.4), 1.0, false, None);
    assert!(close(grabbed, Vec3::new(0.0, 0.5, 0.0)), "{grabbed}");
    // Off, it is left alone.
    let free = grab(block, Vec3::new(0.3, 0.5, 0.4), 0.0, false, None);
    assert!(close(free, Vec3::new(0.3, 0.5, 0.4)));
}

#[test]
fn a_press_on_an_odd_sized_face_snaps_from_its_corner_not_the_world_origin() {
    // 3 studs across X: its corners stand at ±1.5, so the half studs are the
    // grid points along it.
    let block = Mat4::from_scale(Vec3::new(3.0, 1.0, 2.0));
    let grabbed = grab(block, Vec3::new(0.9, 0.5, 0.9), 1.0, false, None);
    assert!(close(grabbed, Vec3::new(0.5, 0.5, 1.0)), "{grabbed}");
}

#[test]
fn a_press_on_a_ball_snaps_on_every_axis_of_its_own_frame() {
    let ball =
        Mat4::from_translation(Vec3::new(0.25, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(4.0));
    let grabbed = grab(ball, Vec3::new(1.9, 1.1, -0.2), 1.0, true, None);
    assert!(close(grabbed, Vec3::new(2.25, 1.0, 0.0)), "{grabbed}");
}

#[test]
fn the_selection_box_of_a_turned_part_reaches_its_corners() {
    let turned = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0))
        * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4)
        * Mat4::from_scale(Vec3::splat(2.0));
    let (centre, size) = selection_box(Mat3::IDENTITY, Vec3::ZERO, [turned]);
    let root2 = std::f32::consts::SQRT_2;
    assert!(close(centre, Vec3::X), "{centre}");
    assert!(
        close(size, Vec3::new(2.0 * root2, 2.0, 2.0 * root2)),
        "{size}"
    );
    // In the part's own frame, it is the part.
    let own = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let (centre, size) = selection_box(own, Vec3::X, [turned]);
    assert!(
        close(centre, Vec3::ZERO) && close(size, Vec3::splat(2.0)),
        "{size}"
    );
}

#[test]
fn the_box_turned_onto_a_face_is_measured_from_the_dragged_point() {
    // A 4 x 2 x 2 box grabbed at the end of its top, turned a quarter about
    // Y: along the frame's X it now spans only 2, its end at the point.
    let turn = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let size = Vec3::new(4.0, 2.0, 2.0);
    let (low, high) = bounds(turn, (Vec3::ZERO, size), Vec3::new(2.0, 1.0, 0.0));
    assert!(close(low, Vec3::new(-1.0, -2.0, 0.0)), "{low}");
    assert!(close(high, Vec3::new(1.0, 0.0, 4.0)), "{high}");
}

#[test]
fn a_snapped_drag_lands_on_the_grid_from_the_faces_nearest_corner() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    let landing = land(&frame, Vec3::new(6.6, 0.5, 7.2), bounds, 1.0, None);
    // 3.4 and 2.8 in from the corner at (10, 0.5, 10) round to 3 and 3.
    assert!(
        close(landing.foot, Vec3::new(7.0, 0.5, 7.0)),
        "{}",
        landing.foot
    );
    // Grabbed by its top, the crate stands its whole height off the face.
    assert!((landing.lift - 2.0).abs() < 1e-5);
    assert!(close(landing.dragged(&frame), Vec3::new(7.0, 2.5, 7.0)));
    assert!(landing.aligned.is_empty());
}

#[test]
fn a_lattice_from_an_odd_sized_faces_corner_is_not_the_world_grid() {
    // A 7-stud-wide face: its corner stands at x = 3.5, so its whole-stud
    // points fall on the half studs of the world.
    let face = surface_frame(
        Mat4::from_scale(Vec3::new(7.0, 1.0, 7.0)),
        Vec3::new(2.0, 0.5, 2.0),
    )
    .unwrap();
    let bounds = grabbed_by_top(&face, Vec3::splat(1.0));
    let landing = land(&face, Vec3::new(1.2, 0.5, 2.1), bounds, 1.0, None);
    assert!(
        close(landing.foot, Vec3::new(1.5, 0.5, 2.5)),
        "{}",
        landing.foot
    );
}

#[test]
fn neither_grid_nor_snap_lands_on_the_cursor() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    let hit = Vec3::new(6.63, 0.5, 7.21);
    let landing = land(&frame, hit, bounds, 0.0, None);
    assert!(close(landing.foot, hit));
}

#[test]
fn with_no_grid_the_box_is_pulled_flush_with_a_face_edge_in_reach() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::new(2.0, 2.0, 2.0));
    // The crate's centre 1.2 in from the x = 10 edge: its side is 0.2 short
    // of it along world X, and its middle a long way off everything else.
    let hit = Vec3::new(8.8, 0.5, 4.3);
    let landing = land(&frame, hit, bounds, 0.0, Some(0.5));
    assert!((landing.foot.x - 9.0).abs() < 1e-4, "{}", landing.foot);
    assert!(
        (landing.foot.z - 4.3).abs() < 1e-4,
        "only the one axis snapped"
    );
    assert_eq!(landing.aligned.len(), 1);
    let [from, to] = landing.aligned[0];
    // Along the plate's x = 10 edge, the whole face long.
    assert!((from.x - 10.0).abs() < 1e-4 && (to.x - 10.0).abs() < 1e-4);
    assert!(((from - to).length() - 20.0).abs() < 1e-3);
}

#[test]
fn out_of_reach_nothing_pulls() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    let hit = Vec3::new(8.2, 0.5, 4.3);
    let landing = land(&frame, hit, bounds, 0.0, Some(0.5));
    assert!(close(landing.foot, hit));
    assert!(landing.aligned.is_empty());
}

#[test]
fn a_soft_snap_beats_the_grid_only_when_it_is_the_smaller_correction() {
    let frame = plate();
    // 1.5 studs wide, so its sides fall between whole studs.
    let bounds = grabbed_by_top(&frame, Vec3::new(1.5, 1.0, 1.5));
    // Centre 0.8 in from the corner along world X: the grid wants +0.2, the
    // box's near side is 0.05 past the edge.
    let landing = land(&frame, Vec3::new(9.2, 0.5, 4.3), bounds, 1.0, Some(0.5));
    assert!((landing.foot.x - 9.25).abs() < 1e-4, "{}", landing.foot);
    assert_eq!(landing.aligned.len(), 1);
    // Z stayed on the grid: 5.7 in from the corner rounds to 6.
    assert!((landing.foot.z - 4.0).abs() < 1e-4);

    // Centre 0.9 in: the grid's +0.1 is smaller than the side's 0.15.
    let landing = land(&frame, Vec3::new(9.1, 0.5, 4.3), bounds, 1.0, Some(0.5));
    assert!((landing.foot.x - 9.0).abs() < 1e-4, "{}", landing.foot);
    assert!(landing.aligned.is_empty());
}

#[test]
fn a_tie_goes_to_the_grid() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    // 1.3 in: the grid wants -0.3, the crate's side is 0.3 past the edge.
    let landing = land(&frame, Vec3::new(8.7, 0.5, 4.0), bounds, 1.0, Some(0.5));
    assert!(landing.aligned.is_empty());
    assert!((landing.foot.x - 9.0).abs() < 1e-4);
}

#[test]
fn the_centre_line_is_an_alignment_too() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    // The crate's middle 0.1 off the plate's own centre line x = 0.
    let landing = land(&frame, Vec3::new(0.1, 0.5, 4.3), bounds, 0.0, Some(0.5));
    assert!(landing.foot.x.abs() < 1e-4, "{}", landing.foot);
}

#[test]
fn a_soft_snapped_drag_draws_its_alignment_lines_and_the_dragged_point() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::new(1.5, 1.0, 1.5));
    let hit = Vec3::new(9.2, 0.5, 4.3);
    let landing = land(&frame, hit, bounds, 1.0, Some(0.5));
    assert_eq!(
        landing.aligned.len(),
        1,
        "soft on one axis, the grid on the other"
    );
    let guides = guides(&frame, hit, &landing, 1.0, true, true, pose(), false);
    // The alignment line, then the dragged point's bar; no ruler.
    assert_eq!(guides.lines.len(), landing.aligned.len() + 1);
    assert_eq!(
        guides.dots.len(),
        1,
        "the dragged point shows while soft-snapped"
    );
    assert!(close(guides.dots[0].centre, landing.dragged(&frame)));
    assert!(guides.lines[1].width > 0.0);
    let line = guides.lines[0];
    assert_eq!((line.color, line.under, line.over), (ACTIVE, 1.0, 0.4));
    // Overrun past both ends of the face by a screen-constant length.
    assert!((line.to - line.from).length() > 20.0);
}

#[test]
fn a_grid_snapped_drag_draws_the_ruler_the_dot_and_its_bar() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    let hit = Vec3::new(6.6, 0.5, 7.2);
    let landing = land(&frame, hit, bounds, 1.0, None);
    let guides = guides(&frame, hit, &landing, 1.0, true, true, pose(), false);
    assert_eq!(guides.dots.len(), 1);
    assert!(close(guides.dots[0].centre, Vec3::new(7.0, 2.5, 7.0)));
    let bar = guides.lines.last().unwrap();
    assert!(bar.width > 0.0 && bar.over == 1.0);
    assert!(close(bar.to, Vec3::new(7.0, 0.5, 7.0)), "down to the face");
    assert!(guides.lines[..guides.lines.len() - 1]
        .iter()
        .all(|line| line.color == ACTIVE && line.under == 1.0 && line.over == 0.5));
}

#[test]
fn each_setting_hides_its_own_part() {
    let frame = plate();
    let bounds = grabbed_by_top(&frame, Vec3::splat(2.0));
    let hit = Vec3::new(6.6, 0.5, 7.2);
    let landing = land(&frame, hit, bounds, 1.0, None);
    let no_target = guides(&frame, hit, &landing, 1.0, false, true, pose(), false);
    assert_eq!(
        (no_target.lines.len(), no_target.dots.len()),
        (1, 1),
        "the bar and dot only"
    );
    let no_point = guides(&frame, hit, &landing, 1.0, true, false, pose(), false);
    assert!(no_point.dots.is_empty() && no_point.lines.iter().all(|line| line.width == 0.0));
    let unsnapped = land(&frame, hit, bounds, 0.0, None);
    assert_eq!(
        guides(&frame, hit, &unsnapped, 0.0, true, true, pose(), false),
        Guides::default()
    );
    assert_ne!(ACTIVE, PASSIVE);
}

/// A 4-stud ball at the origin, framed at the snapped point `at` on it the
/// way `target::ball` frames it.
fn on_ball(at: Vec3) -> SurfaceFrame {
    let normal = at.normalize();
    let z = normal.cross(Vec3::Y).normalize();
    SurfaceFrame {
        corner: at,
        x: normal.cross(z),
        y: normal,
        z,
        size: Vec2::ZERO,
        kind: TargetKind::Sphere,
        part: Some((Solid::Ball, Mat4::from_scale(Vec3::splat(4.0)))),
    }
}

#[test]
fn a_drag_onto_a_ball_lands_on_its_snapped_point_and_aligns_with_nothing() {
    let frame = on_ball(Vec3::new(2.0f32.sqrt(), 1.0, 1.0).normalize() * 2.0);
    let bounds = grabbed_by_top(&frame, Vec3::splat(1.0));
    let hit = frame.corner + frame.x * 0.3 + frame.z * 0.2;
    let landing = land(&frame, hit, bounds, 1.0, Some(5.0));
    assert!(close(landing.foot, frame.corner), "{}", landing.foot);
    assert!(landing.aligned.is_empty());
    let drawn = guides(&frame, hit, &landing, 1.0, true, true, pose(), false);
    // The ball's own guides, its great circles among them, and no ruler.
    assert!(drawn
        .lines
        .iter()
        .any(|line| line.color == PASSIVE && line.under == 1.0 && line.over == 0.0));
    assert!(drawn
        .lines
        .iter()
        .all(|line| !(line.color == ACTIVE && line.over == 0.5)));
}

/// An 8-stud cylinder along X, radius 1, its side framed on `corner` at the
/// +X end facing `y`, with `x` round it.
fn cylinder_side(corner: Vec3, x: Vec3, y: Vec3) -> SurfaceFrame {
    SurfaceFrame {
        corner,
        x,
        y,
        z: Vec3::NEG_X,
        size: Vec2::new(0.0, 8.0),
        kind: TargetKind::Cylinder,
        part: Some((Solid::Cylinder, Mat4::from_scale(Vec3::new(8.0, 2.0, 2.0)))),
    }
}

#[test]
fn a_cylinders_side_takes_the_grid_along_it_but_no_alignment() {
    let frame = cylinder_side(Vec3::new(4.0, 1.0, 0.0), Vec3::Z, Vec3::Y);
    let bounds = grabbed_by_top(&frame, Vec3::splat(1.0));
    let landing = land(&frame, Vec3::new(1.3, 1.0, 0.1), bounds, 1.0, Some(5.0));
    assert!(landing.aligned.is_empty());
    assert!(
        close(landing.foot, Vec3::new(1.0, 1.0, 0.0)),
        "{}",
        landing.foot
    );
}

#[test]
fn a_press_snaps_in_the_frame_the_hover_stood_on() {
    // The cylinder's +Z side, hovered: the grab rounds the distance from
    // its end, not from the box face's corner.
    let side = cylinder_side(Vec3::new(4.0, 0.0, 1.0), Vec3::NEG_Y, Vec3::Z);
    let model = Mat4::from_scale(Vec3::new(8.0, 2.0, 2.0));
    let grabbed = grab(model, Vec3::new(2.4, 0.3, 1.0), 1.0, false, Some(side));
    assert!(close(grabbed, Vec3::new(2.0, 0.0, 1.0)), "{grabbed}");
}
