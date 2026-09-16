use glam::{Mat4, Quat, Vec3};

use super::*;

fn pose(position: Vec3) -> Pose {
    Pose {
        position,
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    }
}

/// A part of `size` studs standing at `centre`, square to the world — the
/// matrix `pick::model_of` builds for one.
fn part(centre: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, centre)
}

/// The handles on a part, seen from ten studs down +Z of the world origin.
fn faces(model: Mat4) -> Faces {
    Faces::new(model, pose(Vec3::new(0.0, 0.0, 10.0)), false)
}

#[test]
fn a_ball_stands_on_the_middle_of_the_face_it_resizes() {
    // A 4×2×6 part at the origin: its faces are 2, 1 and 3 studs out.
    let faces = faces(part(Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0)));

    assert!((faces.handle(Axis::X, 1.0) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4);
    assert!((faces.handle(Axis::X, -1.0) - Vec3::new(-2.0, 0.0, 0.0)).length() < 1e-4);
    assert!((faces.handle(Axis::Y, 1.0) - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4);
    assert!((faces.handle(Axis::Z, -1.0) - Vec3::new(0.0, 0.0, -3.0)).length() < 1e-4);
}

/// The whole point of the change: the handles are bound to the geometry, not
/// to a screen-relative arm out of the pivot. A baseplate's are half a
/// thousand studs out because the plate really is that wide.
#[test]
fn the_handles_grow_with_the_part_rather_than_staying_near_its_pivot() {
    let small = faces(part(Vec3::ZERO, Vec3::splat(4.0)));
    let plate = faces(part(Vec3::ZERO, Vec3::new(1024.0, 16.0, 1024.0)));

    assert!((small.handle(Axis::X, 1.0).x - 2.0).abs() < 1e-3);
    assert!((plate.handle(Axis::X, 1.0).x - 512.0).abs() < 1e-2);
    assert!((plate.handle(Axis::Y, 1.0).y - 8.0).abs() < 1e-3);
}

#[test]
fn the_handles_follow_the_part_as_it_moves() {
    let at = Vec3::new(30.0, -4.0, 12.0);
    let faces = faces(part(at, Vec3::new(2.0, 2.0, 2.0)));

    assert_eq!(faces.centre(), at);
    assert!((faces.handle(Axis::Y, 1.0) - (at + Vec3::Y)).length() < 1e-4);
}

/// `BasePart.Size` runs along the part's own axes and nothing else, so the
/// handles stand on its own faces even for a part turned off the world axes.
#[test]
fn a_turned_parts_handles_stand_on_its_own_faces() {
    let model = Mat4::from_scale_rotation_translation(
        Vec3::new(10.0, 2.0, 2.0),
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Vec3::ZERO,
    );
    let faces = faces(model);

    // A quarter turn about Y swings the part's own X onto world -Z, so its
    // 10-stud length now reaches 5 studs along -Z.
    assert!((faces.direction(Axis::X) - Vec3::NEG_Z).length() < 1e-4);
    assert!((faces.handle(Axis::X, 1.0) - Vec3::new(0.0, 0.0, -5.0)).length() < 1e-3);
    assert!((faces.handle(Axis::X, -1.0) - Vec3::new(0.0, 0.0, 5.0)).length() < 1e-3);
}

#[test]
fn a_part_flattened_to_nothing_still_has_grabbable_handles() {
    // The same defence `basis` makes: a degenerate column must not turn the
    // handle into a NaN one that can never be pointed at.
    let model = Mat4::from_cols(
        Vec3::ZERO.extend(0.0),
        (Vec3::Y * 2.0).extend(0.0),
        (Vec3::Z * 2.0).extend(0.0),
        Vec3::ZERO.extend(1.0),
    );
    let faces = faces(model);

    assert!(faces.handle(Axis::X, 1.0).is_finite());
    assert_eq!(faces.handle(Axis::X, 1.0), Vec3::ZERO);
    assert_eq!(faces.direction(Axis::X), Vec3::X);
}

#[test]
fn a_ball_keeps_its_size_on_screen_rather_than_growing_with_the_part() {
    // Twice as far from the eye is twice as big in studs, which is the same
    // size in pixels — the property that makes a handle on a baseplate's far
    // edge grabbable at all.
    let eye = pose(Vec3::ZERO);
    let near = Faces::new(part(Vec3::new(0.0, 0.0, -20.0), Vec3::ONE), eye, false);
    let far = Faces::new(part(Vec3::new(0.0, 0.0, -40.0), Vec3::ONE), eye, false);

    let ratio = far.radius(Axis::Y, 1.0) / near.radius(Axis::Y, 1.0);
    assert!((ratio - 2.0).abs() < 1e-2, "{ratio}");
}

/// Every ball is measured at its own distance, so the far end of a long part
/// is not drawn (or hit-tested) at the near end's scale.
#[test]
fn each_ball_is_sized_for_where_it_actually_stands() {
    let eye = pose(Vec3::new(0.0, 0.0, 200.0));
    let faces = Faces::new(part(Vec3::ZERO, Vec3::new(2.0, 2.0, 200.0)), eye, false);

    let near = faces.radius(Axis::Z, 1.0);
    let far = faces.radius(Axis::Z, -1.0);
    assert!(far > near * 2.5, "near {near}, far {far}");
}

#[test]
fn pointing_at_a_ball_grabs_that_face() {
    // A 4×2×6 part at the origin, seen down -Z: the +X ball stands at x = 2.
    let faces = faces(part(Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0)));

    let ray = Ray::new(Vec3::new(2.0, 0.0, 30.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(ray), Some((Axis::X, 1.0)));

    let back = Ray::new(Vec3::new(-2.0, 0.0, 30.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(back), Some((Axis::X, -1.0)));

    let up = Ray::new(Vec3::new(0.0, 1.0, 30.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(up), Some((Axis::Y, 1.0)));
}

#[test]
fn the_body_of_the_part_grabs_no_handle() {
    // Between the balls, well inside the part: the Scale tool has nothing to
    // grab there, which is what leaves the click free to fall through to a
    // pick.
    let faces = faces(part(Vec3::ZERO, Vec3::new(40.0, 40.0, 40.0)));
    let ray = Ray::new(Vec3::new(8.0, 8.0, 100.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(ray), None);
}

#[test]
fn a_ball_behind_the_camera_is_not_grabbable() {
    let faces = faces(part(Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0)));
    let away = Ray::new(Vec3::new(2.0, 0.0, -30.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(away), None);
}

#[test]
fn the_nearest_ball_wins_when_two_are_under_the_cursor() {
    // Sighting straight down the Z axis of a part at the origin: the +Z ball
    // is between the eye and the -Z one, and it is the one that can be seen.
    let faces = faces(part(Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0)));
    let ray = Ray::new(Vec3::new(0.0, 0.0, 30.0), Vec3::NEG_Z);
    assert_eq!(faces.grab(ray), Some((Axis::Z, 1.0)));
}

/// What "what you grab is what you see" costs on a part hundreds of studs
/// across: the ball really is out at the edge, and pointing there grabs it.
#[test]
fn a_baseplates_handle_is_grabbed_out_at_its_real_edge() {
    let eye = pose(Vec3::new(0.0, 60.0, 400.0));
    let plate = Faces::new(
        part(Vec3::ZERO, Vec3::new(1024.0, 16.0, 1024.0)),
        eye,
        false,
    );

    let edge = plate.handle(Axis::X, 1.0);
    assert!((edge.x - 512.0).abs() < 1e-2);
    // A ray fired straight at that ball from the eye grabs it, and one aimed
    // at where the old fixed-arm block sat — a few studs off the pivot —
    // grabs nothing at all.
    let at_edge = Ray::new(eye.position, (edge - eye.position).normalize());
    assert_eq!(plate.grab(at_edge), Some((Axis::X, 1.0)));

    let near_pivot = Vec3::new(20.0, 0.0, 0.0);
    let at_pivot = Ray::new(eye.position, (near_pivot - eye.position).normalize());
    assert_eq!(plate.grab(at_pivot), None);
}

#[test]
fn every_face_is_listed_once() {
    let faces = faces(part(Vec3::ZERO, Vec3::ONE));
    let all: Vec<(Axis, f32)> = faces.all().collect();

    assert_eq!(all.len(), 6);
    for axis in Axis::ALL {
        assert!(all.contains(&(axis, 1.0)));
        assert!(all.contains(&(axis, -1.0)));
    }
}
