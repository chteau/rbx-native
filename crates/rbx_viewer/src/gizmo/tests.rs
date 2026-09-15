use glam::{Mat3, Vec3};

use super::*;
use crate::pick::Ray;

fn pose(position: Vec3) -> Pose {
    Pose {
        position,
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    }
}

/// Draggers at the world origin, one stud per arm, on the world axes.
fn handles() -> Handles {
    Handles::new(Vec3::ZERO, basis(None), 1.0)
}

#[test]
fn the_world_basis_is_the_world_axes() {
    assert_eq!(basis(None), [Vec3::X, Vec3::Y, Vec3::Z]);
}

#[test]
fn the_local_basis_follows_the_parts_own_rotation() {
    let rotation = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let [x, y, z] = basis(Some(rotation));

    // A quarter turn about Y swings the part's own X round onto world -Z.
    assert!((x - Vec3::NEG_Z).length() < 1e-4);
    assert!((y - Vec3::Y).length() < 1e-4);
    assert!((z - Vec3::X).length() < 1e-4);
}

#[test]
fn a_local_basis_is_normalized_and_never_degenerate() {
    // A rotation carrying a part's own scale, one axis of it flattened to
    // nothing: the draggers must still point somewhere grabbable.
    let scaled = Mat3::from_cols(Vec3::X * 8.0, Vec3::ZERO, Vec3::Z * 3.0);
    let [x, y, z] = basis(Some(scaled));

    assert!((x.length() - 1.0).abs() < 1e-4);
    assert_eq!(y, Vec3::Y);
    assert!((z.length() - 1.0).abs() < 1e-4);
}

#[test]
fn a_perspective_gizmo_grows_with_its_distance_from_the_eye() {
    let eye = pose(Vec3::ZERO);
    let near = arm_length(Vec3::new(0.0, 0.0, -20.0), eye, false);
    let far = arm_length(Vec3::new(0.0, 0.0, -80.0), eye, false);

    // Four times further away, four times as long: that ratio is what keeps
    // the handles the same size on screen.
    assert!((far / near - 4.0).abs() < 1e-3);
}

#[test]
fn a_gizmo_on_top_of_the_camera_does_not_shrink_to_nothing() {
    let arm = arm_length(Vec3::new(0.0, 0.0, -0.001), pose(Vec3::ZERO), false);
    assert!(arm > 0.1, "an unusable gizmo at {arm} studs");
}

#[test]
fn an_orthographic_gizmo_tracks_the_zoom_rather_than_the_distance() {
    let eye = pose(Vec3::ZERO);
    let near = arm_length(Vec3::new(0.0, 0.0, -20.0), eye, true);
    let far = arm_length(Vec3::new(0.0, 0.0, -800.0), eye, true);

    // Under a parallel projection distance changes nothing on screen, so it
    // must change nothing here either.
    assert_eq!(near, far);
    assert!((near / eye.ortho_scale - 0.2).abs() < 1e-4);
}

#[test]
fn the_closest_point_on_an_axis_is_where_the_cursor_points() {
    // Looking down -Z at a point 3 studs along X: the drag has travelled 3.
    let ray = Ray::new(Vec3::new(3.0, 0.0, 10.0), Vec3::NEG_Z);
    let along = along_axis(Vec3::ZERO, Vec3::X, ray).expect("not parallel");
    assert!((along - 3.0).abs() < 1e-4);

    // Behind the origin, the same in the other direction.
    let back = Ray::new(Vec3::new(-7.5, 0.0, 10.0), Vec3::NEG_Z);
    let along = along_axis(Vec3::ZERO, Vec3::X, back).expect("not parallel");
    assert!((along + 7.5).abs() < 1e-4);
}

#[test]
fn an_axis_seen_end_on_has_no_usable_drag() {
    // Sighting straight down the axis: every point on it projects to the same
    // pixel, so there is no answer to give rather than a wildly noisy one.
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
    assert!(along_axis(Vec3::ZERO, Vec3::Z, ray).is_none());
}

#[test]
fn an_axis_drag_moves_by_the_difference_between_two_samples() {
    let grabbed = Ray::new(Vec3::new(2.0, 0.0, 10.0), Vec3::NEG_Z);
    let moved = Ray::new(Vec3::new(6.5, 0.0, 10.0), Vec3::NEG_Z);

    let from = along_axis(Vec3::ZERO, Vec3::X, grabbed).expect("not parallel");
    let to = along_axis(Vec3::ZERO, Vec3::X, moved).expect("not parallel");
    assert!((to - from - 4.5).abs() < 1e-4);
}

#[test]
fn pointing_at_an_arm_grabs_that_axis() {
    let handles = handles();
    let ray = Ray::new(Vec3::new(0.5, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles.grab(ray), Some(Axis::X));

    let up = Ray::new(Vec3::new(0.0, 0.6, 10.0), Vec3::NEG_Z);
    assert_eq!(handles.grab(up), Some(Axis::Y));
}

#[test]
fn the_arm_on_the_far_side_of_the_origin_grabs_the_same_axis() {
    // Studio's Move gizmo draws an arrow on each end; grabbing either drags
    // along the one line.
    let ray = Ray::new(Vec3::new(-0.5, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles().grab(ray), Some(Axis::X));
}

#[test]
fn the_gap_around_the_origin_grabs_nothing() {
    // What leaves the part itself clickable for a free cursor drag.
    let ray = Ray::new(Vec3::new(0.03, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles().grab(ray), None);
}

#[test]
fn pointing_past_the_end_of_an_arm_grabs_nothing() {
    let ray = Ray::new(Vec3::new(1.4, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles().grab(ray), None);
}

#[test]
fn an_arm_behind_the_camera_is_not_grabbable() {
    // Same geometry as the hit above, but the eye is past the gizmo looking
    // away from it.
    let ray = Ray::new(Vec3::new(0.5, 0.0, -10.0), Vec3::NEG_Z);
    assert_eq!(handles().grab(ray), None);
}

#[test]
fn the_nearest_arm_wins_when_two_are_under_the_cursor() {
    // A ray running along the X/Z diagonal crosses the X arm at 5 studs out
    // and the Z arm at 5 studs out too, dead centre through both. Whichever
    // the user meant, the near one is the one they can see.
    let handles = Handles::new(Vec3::ZERO, basis(None), 10.0);
    let diagonal = Vec3::new(-1.0, 0.0, 1.0).normalize();

    assert_eq!(
        handles.grab(Ray::new(Vec3::new(10.0, 0.0, -5.0), diagonal)),
        Some(Axis::X)
    );
    // The same line walked from the other end meets Z first.
    assert_eq!(
        handles.grab(Ray::new(Vec3::new(-5.0, 0.0, 10.0), -diagonal)),
        Some(Axis::Z)
    );
}

#[test]
fn local_draggers_are_grabbed_where_the_part_actually_points() {
    // A part turned a quarter turn about Y: its own X arm lies along world -Z,
    // so that is where the cursor has to be to grab it.
    let rotation = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 1.0);

    let ray = Ray::new(Vec3::new(10.0, 0.0, -0.5), Vec3::NEG_X);
    assert_eq!(handles.grab(ray), Some(Axis::X));
    assert!((handles.direction(Axis::X) - Vec3::NEG_Z).length() < 1e-4);
}

#[test]
fn each_axis_keeps_its_own_colour() {
    let [red, green, blue] = Axis::ALL.map(Axis::color);
    assert!(red[0] > red[1] && red[0] > red[2]);
    assert!(green[1] > green[0] && green[1] > green[2]);
    assert!(blue[2] > blue[0] && blue[2] > blue[1]);
}

#[test]
fn a_grabbed_arm_reports_which_end_of_the_axis_it_is() {
    let handles = handles();
    let ray = Ray::new(Vec3::new(0.5, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles.grab_arm(ray), Some((Axis::X, 1.0)));

    // The arm on the far side of the origin is the same axis, other end —
    // which is the face a Scale drag grows.
    let back = Ray::new(Vec3::new(-0.5, 0.0, 10.0), Vec3::NEG_Z);
    assert_eq!(handles.grab_arm(back), Some((Axis::X, -1.0)));
}

#[test]
fn a_rings_frame_turns_the_right_way_about_its_own_axis() {
    let handles = handles();
    for axis in Axis::ALL {
        let (normal, zero, quarter) = handles.ring_frame(axis);

        assert!((normal - handles.direction(axis)).length() < 1e-4);
        assert!(zero.dot(normal).abs() < 1e-4, "the zero left the plane");
        assert!(quarter.dot(normal).abs() < 1e-4, "the quarter left it");
        // Walking a quarter turn the way this frame measures has to be a
        // *positive* rotation about the axis, or every drag would turn the
        // part backwards.
        let turned = Mat3::from_axis_angle(normal, std::f32::consts::FRAC_PI_2) * zero;
        assert!((turned - quarter).length() < 1e-4);
    }
}

#[test]
fn a_ray_crossing_a_ring_reports_where_round_it_landed() {
    let handles = handles();
    let frame = handles.ring_frame(Axis::Z);
    // The Z ring lies in the world's XY plane, with its zero on X.
    let (angle, radius, distance) = ring_crossing(
        Vec3::ZERO,
        frame,
        Ray::new(Vec3::new(3.0, 0.0, 10.0), Vec3::NEG_Z),
    )
    .expect("the ray crosses the ring's plane");
    assert!(angle.abs() < 1e-4, "{angle} is not the ring's zero");
    assert!((radius - 3.0).abs() < 1e-4);
    assert!((distance - 10.0).abs() < 1e-4);

    // A quarter of the way round, which is the frame's second direction.
    let (angle, ..) = ring_crossing(
        Vec3::ZERO,
        frame,
        Ray::new(Vec3::new(0.0, 3.0, 10.0), Vec3::NEG_Z),
    )
    .expect("the ray crosses the ring's plane");
    assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
}

#[test]
fn a_ray_along_a_rings_plane_has_no_angle_rather_than_a_wild_one() {
    let handles = handles();
    let along = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Y);
    assert_eq!(
        ring_crossing(Vec3::ZERO, handles.ring_frame(Axis::Y), along),
        None
    );
}

#[test]
fn pointing_at_a_ring_grabs_that_axis() {
    let handles = handles();
    // Straight down -Z at a point on the Z ring's own circle, one arm out.
    assert_eq!(
        handles.grab_ring(Ray::new(Vec3::new(1.0, 0.0, 10.0), Vec3::NEG_Z)),
        Some(Axis::Z)
    );
    // And the same ring from the other side of the circle.
    assert_eq!(
        handles.grab_ring(Ray::new(Vec3::new(0.0, -1.0, 10.0), Vec3::NEG_Z)),
        Some(Axis::Z)
    );
}

#[test]
fn the_middle_of_a_ring_and_the_space_outside_it_grab_nothing() {
    let handles = handles();
    // Dead centre: inside every ring, on none of them. The X and Y rings are
    // edge-on to this ray and so have no crossing at all.
    assert_eq!(
        handles.grab_ring(Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z)),
        None
    );
    assert_eq!(
        handles.grab_ring(Ray::new(Vec3::new(1.4, 0.0, 10.0), Vec3::NEG_Z)),
        None
    );
}

#[test]
fn a_ring_behind_the_camera_is_not_grabbable() {
    let handles = handles();
    let away = Ray::new(Vec3::new(1.0, 0.0, -10.0), Vec3::NEG_Z);
    assert_eq!(handles.grab_ring(away), None);
}

#[test]
fn local_rings_are_grabbed_where_the_part_actually_faces() {
    // A part turned a quarter turn about Y: its own Z axis now lies on world
    // X, so its Z ring stands in the world's YZ plane.
    let rotation = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 1.0);

    let ray = Ray::new(Vec3::new(10.0, 1.0, 0.0), Vec3::NEG_X);
    assert_eq!(handles.grab_ring(ray), Some(Axis::Z));
    assert!((handles.direction(Axis::Z) - Vec3::X).length() < 1e-4);
}

#[test]
fn an_angle_step_always_goes_the_short_way_round() {
    use std::f32::consts::{PI, TAU};

    assert!((angle_step(0.1, 0.4) - 0.3).abs() < 1e-5);
    assert!((angle_step(0.4, 0.1) + 0.3).abs() < 1e-5);
    // Across the seam at ±π: a hair over, not a whole turn back.
    let step = angle_step(PI - 0.05, -PI + 0.05);
    assert!((step - 0.1).abs() < 1e-4, "{step} the long way round");
    let back = angle_step(-PI + 0.05, PI - 0.05);
    assert!((back + 0.1).abs() < 1e-4, "{back} the long way round");
    // And never outside one half turn, whatever it is handed.
    for turns in -4..=4 {
        let step = angle_step(0.0, 0.3 + TAU * turns as f32);
        assert!(step.abs() <= PI + 1e-4);
    }
}

/// A part of `size` studs standing at `centre`, axis-aligned — the matrix
/// `pick::model_of` builds for one.
fn part(centre: Vec3, size: Vec3) -> glam::Mat4 {
    glam::Mat4::from_scale_rotation_translation(size, glam::Quat::IDENTITY, centre)
}

#[test]
fn one_part_centres_the_gizmo_on_that_part() {
    // A single selection has to give the same answer it always did, so this
    // needs no special case anywhere above it.
    let at = Vec3::new(3.0, -1.0, 7.0);
    let centre = centre_of([part(at, Vec3::new(4.0, 2.0, 6.0))]).expect("one part");
    assert!((centre - at).length() < 1e-4, "{centre:?}");
}

#[test]
fn an_empty_selection_has_no_centre() {
    assert_eq!(centre_of([]), None);
}

#[test]
fn two_parts_put_the_gizmo_between_them() {
    let centre = centre_of([
        part(Vec3::new(-10.0, 0.0, 0.0), Vec3::ONE),
        part(Vec3::new(10.0, 0.0, 0.0), Vec3::ONE),
    ])
    .expect("two parts");
    assert!((centre - Vec3::ZERO).length() < 1e-4, "{centre:?}");
}

#[test]
fn the_centre_is_of_the_bounds_not_the_mean_of_the_parts() {
    // Three small parts bunched at one end and one large at the other: the
    // mean of the centres would sit among the bunch, the centre of the bounds
    // sits where the selection actually looks centred.
    let models = [
        part(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE),
        part(Vec3::new(1.0, 0.0, 0.0), Vec3::ONE),
        part(Vec3::new(2.0, 0.0, 0.0), Vec3::ONE),
        part(Vec3::new(20.0, 0.0, 0.0), Vec3::ONE),
    ];
    let mean = (0.0 + 1.0 + 2.0 + 20.0) / 4.0;
    let centre = centre_of(models).expect("four parts");
    // Bounds run -0.5 .. 20.5, so the centre is 10.
    assert!((centre.x - 10.0).abs() < 1e-4, "{centre:?}");
    assert!(
        (centre.x - mean).abs() > 1.0,
        "the two must not coincide here"
    );
}

#[test]
fn a_parts_own_size_counts_towards_the_bounds() {
    // Both centred on the origin's line, but the wide one reaches further, so
    // the centre is pulled towards its far face rather than sitting between
    // the two centres.
    let centre = centre_of([
        part(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE),
        part(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 1.0, 1.0)),
    ])
    .expect("two parts");
    // Bounds run -0.5 .. 20.0, so the centre is 9.75.
    assert!((centre.x - 9.75).abs() < 1e-4, "{centre:?}");
}

#[test]
fn a_turned_part_is_contained_by_the_world_axes_it_actually_spans() {
    // A 2×2×2 cube turned 45° about Y reaches 2·√2/2 ≈ 1.414 along world X and
    // Z, not 1: clamping to its own axes would under-measure the bounds and
    // put the gizmo off-centre.
    let turned = glam::Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
        Vec3::ZERO,
    );
    let flat = part(Vec3::new(10.0, 0.0, 0.0), Vec3::ONE);
    let centre = centre_of([turned, flat]).expect("two parts");
    // Bounds run -√2 .. 10.5 on X.
    let expected = (-std::f32::consts::SQRT_2 + 10.5) * 0.5;
    assert!((centre.x - expected).abs() < 1e-3, "{centre:?}");
}

#[test]
fn a_quarter_turn_about_the_camera_right_vector_tilts_towards_the_camera() {
    // A camera looking down -Z has +Z towards it and X to its right; tilting
    // the part's top towards the camera therefore carries +Y to +Z.
    let tilted = quarter_turn(Vec3::X) * Vec3::Y;
    assert!((tilted - Vec3::Z).length() < 1e-5, "{tilted:?}");
}

#[test]
fn a_quarter_turn_is_a_quarter_turn_whatever_the_axis_length() {
    let unit = quarter_turn(Vec3::Y);
    let long = quarter_turn(Vec3::Y * 17.0);
    assert!((unit.x_axis - long.x_axis).length() < 1e-5);
    assert!((unit.z_axis - long.z_axis).length() < 1e-5);
}

#[test]
fn a_degenerate_turn_axis_falls_back_to_the_world_up() {
    // Same defence as `basis`: a zero axis would otherwise produce a NaN
    // rotation that silently destroys the part's `CFrame`.
    let turn = quarter_turn(Vec3::ZERO);
    assert!(turn.is_finite());
    assert!((turn * Vec3::Z - Vec3::X).length() < 1e-5);
}

#[test]
fn turning_about_a_pivot_leaves_the_pivot_itself_where_it_was() {
    let pivot = Vec3::new(3.0, 1.0, -2.0);
    let (_, position) = turned(Mat3::IDENTITY, pivot, pivot, quarter_turn(Vec3::Y));
    assert!((position - pivot).length() < 1e-5, "{position:?}");
}

#[test]
fn turning_carries_the_part_around_the_grab_point() {
    // Grabbed at the origin, standing two studs along +X: a quarter turn about
    // +Y carries it to -Z (right-handed), and its own axes turn with it.
    let turn = quarter_turn(Vec3::Y);
    let (linear, position) = turned(Mat3::IDENTITY, Vec3::X * 2.0, Vec3::ZERO, turn);
    assert!(
        (position - Vec3::NEG_Z * 2.0).length() < 1e-5,
        "{position:?}"
    );
    assert!((linear.x_axis - Vec3::NEG_Z).length() < 1e-5, "{linear:?}");
}

#[test]
fn turning_keeps_a_parts_size_in_its_columns() {
    // The matrix the draggers are drawn from still carries `Size` in its
    // column lengths; a turn has to rotate it without rescaling it.
    let linear = Mat3::from_diagonal(Vec3::new(4.0, 1.0, 2.0));
    let (spun, _) = turned(linear, Vec3::ZERO, Vec3::ZERO, quarter_turn(Vec3::Y));
    assert!((spun.x_axis.length() - 4.0).abs() < 1e-5);
    assert!((spun.y_axis.length() - 1.0).abs() < 1e-5);
    assert!((spun.z_axis.length() - 2.0).abs() < 1e-5);
}

#[test]
fn four_quarter_turns_come_back_to_where_they_started() {
    let turn = quarter_turn(Vec3::new(1.0, 1.0, 0.0));
    let start = (Mat3::IDENTITY, Vec3::new(5.0, 0.0, 0.0));
    let pivot = Vec3::new(1.0, 2.0, 3.0);
    let mut state = start;
    for _ in 0..4 {
        state = turned(state.0, state.1, pivot, turn);
    }
    assert!((state.1 - start.1).length() < 1e-4, "{:?}", state.1);
    assert!((state.0.x_axis - Vec3::X).length() < 1e-4, "{:?}", state.0);
}
