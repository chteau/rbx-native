use glam::{Mat4, Quat, Vec3};

use super::*;

/// A part of `size` studs standing at `centre`, axis-aligned — the matrix
/// `pick::model_of` builds for one.
fn part(centre: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, centre)
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn rounding_goes_to_the_nearest_multiple_in_both_directions() {
    assert_eq!(round_to(1.2, 1.0), 1.0);
    assert_eq!(round_to(1.7, 1.0), 2.0);
    assert_eq!(round_to(-1.2, 1.0), -1.0);
    assert_eq!(round_to(-1.7, 1.0), -2.0);
    assert_eq!(round_to(1.3, 0.5), 1.5);
    assert_eq!(round_to(-1.3, 0.5), -1.5);
}

#[test]
fn a_value_exactly_between_two_increments_rounds_away_from_zero() {
    // The boundary case: 0.5 of an increment has two equally near answers, and
    // which one it takes has to be settled rather than left to drift.
    assert_eq!(round_to(0.5, 1.0), 1.0);
    assert_eq!(round_to(-0.5, 1.0), -1.0);
    assert_eq!(round_to(1.5, 1.0), 2.0);
    assert_eq!(round_to(0.25, 0.5), 0.5);
}

#[test]
fn an_increment_of_zero_or_less_is_no_grid_at_all() {
    // The toolbar's field takes whatever is typed; "0 studs" has to mean "no
    // snapping" rather than a division by zero.
    assert_eq!(round_to(1.234, 0.0), 1.234);
    assert_eq!(round_to(1.234, -1.0), 1.234);
    assert_eq!(round_to(1.234, f32::NAN), 1.234);
    assert_eq!(round_to(1.234, f32::INFINITY), 1.234);
}

#[test]
fn rounding_a_point_rounds_every_component() {
    assert_eq!(
        round_point(Vec3::new(1.2, -1.7, 0.4), 1.0),
        Vec3::new(1.0, -2.0, 0.0)
    );
}

#[test]
fn nothing_within_reach_soft_snaps_to_nothing() {
    let boxes = [part(Vec3::ZERO, Vec3::splat(4.0))];
    assert_eq!(
        nearest_surface(Vec3::new(0.0, 20.0, 0.0), &boxes, 0.5),
        None
    );
}

#[test]
fn an_empty_world_soft_snaps_to_nothing() {
    assert_eq!(nearest_surface(Vec3::ZERO, &[], 1.0), None);
}

#[test]
fn a_point_just_above_a_face_is_pulled_onto_it() {
    // Well inside the face, far from every edge: the point lands on the
    // surface directly below it, not on a corner.
    let boxes = [part(Vec3::ZERO, Vec3::splat(10.0))];
    let surface = nearest_surface(Vec3::new(1.0, 5.3, -1.0), &boxes, 0.5).expect("in reach");
    assert!(
        close(surface.point, Vec3::new(1.0, 5.0, -1.0)),
        "{surface:?}"
    );
    assert!(close(surface.normal, Vec3::Y), "{surface:?}");
}

#[test]
fn the_reach_is_measured_in_studs_not_in_box_widths() {
    // A long thin part: clamping into unit-cube space would make the same
    // stud offset read as a different distance on each axis.
    let boxes = [part(Vec3::ZERO, Vec3::new(100.0, 1.0, 1.0))];
    assert!(nearest_surface(Vec3::new(0.0, 0.9, 0.0), &boxes, 0.5).is_some());
    assert!(nearest_surface(Vec3::new(0.0, 1.2, 0.0), &boxes, 0.5).is_none());
}

#[test]
fn a_point_near_two_faces_at_once_is_pulled_onto_their_edge() {
    // The top face and the +X face are both within reach, so the answer is the
    // edge they share rather than either face on its own.
    let boxes = [part(Vec3::ZERO, Vec3::splat(10.0))];
    let surface = nearest_surface(Vec3::new(4.8, 5.2, 0.0), &boxes, 0.5).expect("in reach");
    assert!(
        close(surface.point, Vec3::new(5.0, 5.0, 0.0)),
        "{surface:?}"
    );
}

#[test]
fn a_point_near_three_faces_at_once_is_pulled_onto_their_corner() {
    let boxes = [part(Vec3::ZERO, Vec3::splat(10.0))];
    let surface = nearest_surface(Vec3::new(4.8, 5.2, 4.9), &boxes, 0.5).expect("in reach");
    assert!(close(surface.point, Vec3::splat(5.0)), "{surface:?}");
}

#[test]
fn a_point_inside_a_part_is_pushed_out_through_its_nearest_face() {
    let boxes = [part(Vec3::ZERO, Vec3::new(10.0, 2.0, 10.0))];
    let surface = nearest_surface(Vec3::new(0.0, 0.8, 0.0), &boxes, 1.0).expect("in reach");
    assert!(
        close(surface.point, Vec3::new(0.0, 1.0, 0.0)),
        "{surface:?}"
    );
    assert!(close(surface.normal, Vec3::Y), "{surface:?}");
}

#[test]
fn the_nearest_of_several_parts_wins() {
    let boxes = [
        part(Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0)),
        part(Vec3::new(0.0, 3.0, 0.0), Vec3::splat(2.0)),
    ];
    // 1.4 is 0.4 above the lower part's top face and 0.6 below the upper
    // part's bottom one.
    let surface = nearest_surface(Vec3::new(0.0, 1.4, 0.0), &boxes, 1.0).expect("in reach");
    assert!((surface.point.y - 1.0).abs() < 1e-4, "{surface:?}");
}

#[test]
fn a_rotated_part_snaps_to_its_own_turned_surface() {
    // Turned 45° about Y, so its +X face's normal points diagonally and its
    // surface is nowhere near an axis-aligned plane.
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let model = Mat4::from_scale_rotation_translation(Vec3::splat(2.0), turn, Vec3::ZERO);
    let normal = (turn * Vec3::X).normalize();
    let just_outside = normal * 1.2;
    let surface = nearest_surface(just_outside, &[model], 0.5).expect("in reach");
    assert!(close(surface.normal, normal), "{surface:?}");
    assert!(
        (surface.point.length() - normal.length()).abs() < 1e-3,
        "{surface:?}"
    );
}

#[test]
fn a_part_scaled_to_nothing_has_no_surface_to_snap_to() {
    let flat =
        Mat4::from_scale_rotation_translation(Vec3::new(4.0, 0.0, 4.0), Quat::IDENTITY, Vec3::ZERO);
    assert_eq!(
        nearest_surface(Vec3::new(0.0, 0.1, 0.0), &[flat], 1.0),
        None
    );
}
