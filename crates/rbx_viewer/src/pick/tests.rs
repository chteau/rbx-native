use glam::{Mat4, Vec2, Vec3};
use rbx_dom::{CFrameData, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::Pose;

fn test_place() -> WeakDom {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let bytes = std::fs::read(&path).expect("fixture must be readable");
    rbx_binary::deserialize(&bytes).expect("fixture must parse")
}

const FOV: f32 = 70.0;

fn pose(position: Vec3, yaw: f32, pitch: f32) -> Pose {
    Pose {
        position,
        yaw,
        pitch,
        fov_degrees: FOV,
        ortho_scale: 20.0,
    }
}

fn identity_cframe(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

#[test]
fn ndc_spans_minus_one_to_one_with_y_flipped() {
    let size = Vec2::new(800.0, 600.0);
    assert_eq!(ndc_of(Vec2::new(400.0, 300.0), size), Vec2::ZERO);
    // Top-left pixel is NDC (-1, +1): the y flip is the whole point.
    assert_eq!(ndc_of(Vec2::ZERO, size), Vec2::new(-1.0, 1.0));
    assert_eq!(ndc_of(size, size), Vec2::new(1.0, -1.0));
}

#[test]
fn a_zero_sized_viewport_does_not_divide_by_zero() {
    let ndc = ndc_of(Vec2::ZERO, Vec2::ZERO);
    assert!(ndc.x.is_finite() && ndc.y.is_finite());
}

#[test]
fn the_centre_ray_runs_straight_down_the_view_axis() {
    let pose = pose(Vec3::new(3.0, 4.0, 5.0), 0.0, 0.0);
    let ray = ray_through(pose.view_projection(false, 16.0 / 9.0), Vec2::ZERO);

    // Yaw and pitch of zero look down -Z (see `camera::direction`).
    assert!((ray.direction - Vec3::NEG_Z).length() < 1e-4);
    // The ray starts on the near plane, which is directly in front of the eye.
    assert!((ray.origin - pose.position).length() < 0.1);
}

#[test]
fn an_edge_ray_leaves_at_half_the_field_of_view() {
    let pose = pose(Vec3::ZERO, 0.0, 0.0);
    // A square frame, so the horizontal half-angle is the vertical one.
    let projection = pose.view_projection(false, 1.0);

    let right = ray_through(projection, Vec2::new(1.0, 0.0));
    assert!(right.direction.x > 0.0);
    assert!((right.direction.angle_between(Vec3::NEG_Z).to_degrees() - FOV * 0.5).abs() < 1e-2);

    let up = ray_through(projection, Vec2::new(0.0, 1.0));
    assert!(up.direction.y > 0.0);
    assert!((up.direction.angle_between(Vec3::NEG_Z).to_degrees() - FOV * 0.5).abs() < 1e-2);
}

#[test]
fn a_yawed_camera_turns_its_rays_with_it() {
    let turned = pose(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0);
    let ray = ray_through(turned.view_projection(false, 1.0), Vec2::ZERO);
    // Yawing a quarter turn swings the view from -Z round to -X.
    assert!((ray.direction - Vec3::NEG_X).length() < 1e-4);
}

#[test]
fn orthographic_rays_stay_parallel_and_move_their_origin_instead() {
    let pose = pose(Vec3::ZERO, 0.0, 0.0);
    let projection = pose.view_projection(true, 1.0);

    let centre = ray_through(projection, Vec2::ZERO);
    let right = ray_through(projection, Vec2::new(1.0, 0.0));

    assert!((centre.direction - right.direction).length() < 1e-4);
    // Half the view volume's width away, which at aspect 1 is `ortho_scale`.
    assert!((right.origin.x - centre.origin.x - pose.ortho_scale).abs() < 1e-2);
}

#[test]
fn a_ray_meets_a_unit_box_at_its_near_face() {
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
    let hit = ray_hits_box(ray, Mat4::IDENTITY).expect("the ray points at the box");
    assert!((hit - 9.5).abs() < 1e-4);
}

#[test]
fn a_ray_that_misses_reports_nothing() {
    let ray = Ray::new(Vec3::new(5.0, 0.0, 10.0), Vec3::NEG_Z);
    assert!(ray_hits_box(ray, Mat4::IDENTITY).is_none());
}

#[test]
fn a_box_behind_the_ray_is_not_a_hit() {
    // Pointing away from a box that sits behind the origin: selecting by
    // clicking must never reach something the camera has already flown past.
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::Z);
    assert!(ray_hits_box(ray, Mat4::IDENTITY).is_none());
}

#[test]
fn a_ray_starting_inside_the_box_hits_at_zero() {
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    assert_eq!(ray_hits_box(ray, Mat4::IDENTITY), Some(0.0));
}

#[test]
fn the_box_follows_its_model_matrix() {
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0))
        * Mat4::from_scale(Vec3::new(4.0, 4.0, 2.0));
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    let hit = ray_hits_box(ray, model).expect("the ray points at the box");
    // Centre 20 studs away, half a stud of its own depth in front of that.
    assert!((hit - 19.0).abs() < 1e-4);

    // Just outside the scaled box's half-width of 2 studs.
    let past = Ray::new(Vec3::new(2.5, 0.0, 0.0), Vec3::NEG_Z);
    assert!(ray_hits_box(past, model).is_none());
}

#[test]
fn a_rotated_box_is_hit_on_its_own_axes() {
    // Turned 45 degrees about Y, a 1-stud cube's corner reaches further along
    // Z than its face did: the hit has to be tested in the box's frame, not
    // the world's.
    let model = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
    let hit = ray_hits_box(ray, model).expect("the ray points at the box");
    assert!((hit - (10.0 - 0.5 * std::f32::consts::SQRT_2)).abs() < 1e-4);
}

#[test]
fn a_part_flattened_to_nothing_is_not_pickable() {
    let model = Mat4::from_scale(Vec3::new(4.0, 0.0, 4.0));
    let ray = Ray::new(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y);
    assert!(ray_hits_box(ray, model).is_none());
}

#[test]
fn a_part_model_carries_its_position_and_size() {
    let model = part_model(
        &identity_cframe(10.0, 2.0, -3.0),
        Vector3Data {
            x: 4.0,
            y: 1.0,
            z: 2.0,
        },
    );

    assert!((model.transform_point3(Vec3::ZERO) - Vec3::new(10.0, 2.0, -3.0)).length() < 1e-4);
    // The unit cube's corner lands half a size away on every axis.
    let corner = model.transform_point3(Vec3::splat(0.5));
    assert!((corner - Vec3::new(12.0, 2.5, -2.0)).length() < 1e-4);
}

#[test]
fn a_ray_meets_a_plane_where_it_crosses_it() {
    let ray = Ray::new(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y);
    let hit = ray_hits_plane(ray, Vec3::new(5.0, 3.0, 5.0), Vec3::Y).expect("it crosses");
    assert!((hit - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4);
}

#[test]
fn a_click_down_at_the_place_finds_its_parts_nearest_first() {
    // TestPlace is a 2048x16x2048 baseplate centred at y=-8 with a 12x1x12
    // spawn standing on it at y=0.5, so a ray straight down the middle meets
    // the spawn first and the baseplate behind it.
    let dom = test_place();
    let database = ReflectionDatabase::embedded();
    let down = Ray::new(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y);

    let hits = parts_along(&dom, &database, down);
    assert_eq!(hits.len(), 2, "the spawn and the baseplate");

    let names: Vec<&str> = hits
        .iter()
        .map(|&referent| dom.get(referent).expect("a hit resolves").name())
        .collect();
    assert_eq!(names[0], "SpawnLocation");
    assert_eq!(names[1], "Baseplate");
}

#[test]
fn a_click_off_the_edge_of_the_place_finds_nothing() {
    let dom = test_place();
    let database = ReflectionDatabase::embedded();
    let past = Ray::new(Vec3::new(5000.0, 200.0, 0.0), Vec3::NEG_Y);

    assert!(parts_along(&dom, &database, past).is_empty());
}

#[test]
fn an_empty_dom_is_clickable_without_panicking() {
    let database = ReflectionDatabase::embedded();
    let down = Ray::new(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y);

    assert!(parts_along(&WeakDom::new(), &database, down).is_empty());
}

#[test]
fn a_ray_parallel_to_a_plane_never_meets_it() {
    let ray = Ray::new(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Z);
    assert!(ray_hits_plane(ray, Vec3::ZERO, Vec3::Y).is_none());
}
