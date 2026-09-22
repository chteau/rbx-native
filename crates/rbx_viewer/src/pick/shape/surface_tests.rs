use glam::{Mat4, Vec3};

use super::*;

#[track_caller]
fn assert_surface(actual: Option<(Vec3, Vec3)>, point: Vec3, normal: Vec3) {
    let (at, facing) = actual.expect("the ray meets the solid");
    assert!(
        (at - point).length() < 1e-4,
        "hit at {at}, expected {point}"
    );
    let normal = normal.normalize();
    assert!(
        (facing - normal).length() < 1e-4,
        "normal {facing}, expected {normal}"
    );
}

fn down_at(x: f32, z: f32) -> Ray {
    Ray::new(Vec3::new(x, 10.0, z), Vec3::NEG_Y)
}

#[test]
fn a_box_face_is_its_own_axis() {
    let ray = down_at(0.1, 0.2);
    assert_surface(
        surface(ShapeKind::Box, Mat4::IDENTITY, ray),
        Vec3::new(0.1, 0.5, 0.2),
        Vec3::Y,
    );
}

// The slope climbs from the front-bottom edge to the back-top one: straight
// down onto it lands on `y = z`, facing up and out the front.
#[test]
fn a_wedge_is_hit_on_its_slope_not_its_box_top() {
    assert_surface(
        surface(ShapeKind::Wedge, Mat4::IDENTITY, down_at(0.0, -0.25)),
        Vec3::new(0.0, -0.25, -0.25),
        Vec3::new(0.0, 1.0, -1.0),
    );
}

// Four studs deep and one tall, the slope rises a quarter stud per stud: its
// normal leans a quarter as far forward, which only the inverse-transpose of
// the stretch gives — the stretch itself would lean it four times as far.
#[test]
fn a_stretched_wedge_tilts_its_slope_normal_the_other_way() {
    let model = Mat4::from_scale(Vec3::new(1.0, 1.0, 4.0));
    assert_surface(
        surface(ShapeKind::Wedge, model, down_at(0.0, -1.0)),
        Vec3::new(0.0, -0.25, -1.0),
        Vec3::new(0.0, 1.0, -0.25),
    );
}

#[test]
fn a_corner_wedge_is_hit_on_whichever_slope_is_above() {
    assert_surface(
        surface(ShapeKind::CornerWedge, Mat4::IDENTITY, down_at(0.25, 0.25)),
        Vec3::new(0.25, -0.25, 0.25),
        Vec3::new(0.0, 1.0, 1.0),
    );
}

#[test]
fn a_ball_faces_out_from_its_centre() {
    let ray = Ray::new(Vec3::new(10.0, 0.3, 0.0), Vec3::NEG_X);
    let x = (0.25f32 - 0.09).sqrt();
    assert_surface(
        surface(ShapeKind::Ball, Mat4::IDENTITY, ray),
        Vec3::new(x, 0.3, 0.0),
        Vec3::new(x, 0.3, 0.0),
    );
}

#[test]
fn a_cylinder_faces_out_from_its_axis_on_the_side_and_along_it_on_a_cap() {
    // Lying along X: from above, the curved side at an off-axis `z`.
    let z = 0.3;
    let y = (0.25f32 - z * z).sqrt();
    assert_surface(
        surface(ShapeKind::CylinderX, Mat4::IDENTITY, down_at(0.1, z)),
        Vec3::new(0.1, y, z),
        Vec3::new(0.0, y, z),
    );
    let end_on = Ray::new(Vec3::new(10.0, 0.1, 0.1), Vec3::NEG_X);
    assert_surface(
        surface(ShapeKind::CylinderX, Mat4::IDENTITY, end_on),
        Vec3::new(0.5, 0.1, 0.1),
        Vec3::X,
    );
}

#[test]
fn a_ray_from_inside_enters_through_no_face() {
    let ray = Ray::new(Vec3::ZERO, Vec3::X);
    assert!(surface(ShapeKind::Box, Mat4::IDENTITY, ray).is_none());
    assert!(hit(ShapeKind::Box, Mat4::IDENTITY, ray).is_some());
}
