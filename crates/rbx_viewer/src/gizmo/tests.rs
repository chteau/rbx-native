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
