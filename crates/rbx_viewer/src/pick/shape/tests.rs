use glam::{EulerRot, Mat4, Quat, Vec3};

use super::*;
use crate::pick::mesh;
use crate::shapes::{self, MeshData};

fn toward_neg_z(x: f32, y: f32) -> Ray {
    Ray::new(Vec3::new(x, y, 10.0), Vec3::NEG_Z)
}

fn toward_neg_x(y: f32, z: f32) -> Ray {
    Ray::new(Vec3::new(10.0, y, z), Vec3::NEG_X)
}

fn toward_neg_y(x: f32, z: f32) -> Ray {
    Ray::new(Vec3::new(x, 10.0, z), Vec3::NEG_Y)
}

fn unit(kind: ShapeKind, ray: Ray) -> Option<f32> {
    hit(kind, Mat4::IDENTITY, ray)
}

fn box_only(ray: Ray) -> Option<f32> {
    unit(ShapeKind::Box, ray)
}

#[track_caller]
fn assert_close(actual: Option<f32>, expected: f32) {
    let actual = actual.expect("the ray meets the solid");
    assert!(
        (actual - expected).abs() < 1e-4,
        "hit at {actual}, expected {expected}"
    );
}

/// Turned by three arbitrary angles, stretched differently on every axis and
/// moved off the origin: nothing about it lines up with the world.
fn arbitrary_model() -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_euler(EulerRot::XYZ, 0.7, -1.1, 0.4),
        Vec3::new(3.0, -2.0, 5.0),
    ) * Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0))
}

// --- Ball -----------------------------------------------------------------

#[test]
fn a_ball_is_missed_at_the_corner_of_its_box() {
    // Inside the unit cube's footprint, outside the half-stud radius.
    let corner = toward_neg_z(0.45, 0.45);
    assert!(box_only(corner).is_some(), "the box test used to say hit");
    assert!(unit(ShapeKind::Ball, corner).is_none());
}

#[test]
fn a_ball_is_hit_on_its_curve_not_its_box_face() {
    // At x = 0.3 the sphere's surface sits at z = sqrt(0.25 - 0.09) = 0.4,
    // a tenth of a stud behind the box face the old test would report.
    let ray = toward_neg_z(0.3, 0.0);
    assert_close(box_only(ray), 9.5);
    assert_close(unit(ShapeKind::Ball, ray), 9.6);
}

#[test]
fn a_ball_scaled_and_moved_keeps_its_radius() {
    let model =
        Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0)) * Mat4::from_scale(Vec3::splat(4.0));
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    // Centre 20 studs away, radius 2.
    assert_close(hit(ShapeKind::Ball, model, ray), 18.0);
    // Just outside the radius, still inside the 4-stud box.
    let past = Ray::new(Vec3::new(1.5, 1.5, 0.0), Vec3::NEG_Z);
    assert!(hit(ShapeKind::Box, model, past).is_some());
    assert!(hit(ShapeKind::Ball, model, past).is_none());
}

#[test]
fn a_ray_inside_a_ball_hits_at_zero_and_one_behind_it_misses() {
    assert_eq!(
        unit(ShapeKind::Ball, Ray::new(Vec3::ZERO, Vec3::NEG_Z)),
        Some(0.0)
    );
    let behind = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::Z);
    assert!(unit(ShapeKind::Ball, behind).is_none());
}

#[test]
fn a_ball_that_misses_entirely_reports_nothing() {
    assert!(unit(ShapeKind::Ball, toward_neg_z(3.0, 0.0)).is_none());
}

// --- Cylinder -------------------------------------------------------------

#[test]
fn a_part_cylinder_lies_along_x() {
    // Straight down the axis: the flat cap, half a stud out.
    assert_close(unit(ShapeKind::CylinderX, toward_neg_x(0.0, 0.0)), 9.5);
    // At the cap's corner the box has material and the disc does not.
    let corner = toward_neg_x(0.45, 0.45);
    assert!(box_only(corner).is_some());
    assert!(unit(ShapeKind::CylinderX, corner).is_none());
    // Across the axis the curved side is met before the box face would be:
    // at z = 0.45 the surface stands at y = sqrt(0.25 - 0.2025).
    let across = toward_neg_y(0.0, 0.45);
    assert_close(unit(ShapeKind::CylinderX, across), 10.0 - 0.217_944_95);
}

#[test]
fn a_mesh_cylinder_stands_along_y() {
    // The same rays as the X case answer the other way round, which is what
    // tells the two axes apart: down Y is now the flat cap.
    assert_close(unit(ShapeKind::CylinderY, toward_neg_y(0.0, 0.45)), 9.5);
    let corner = toward_neg_y(0.45, 0.45);
    assert!(box_only(corner).is_some());
    assert!(unit(ShapeKind::CylinderY, corner).is_none());
    assert_close(
        unit(ShapeKind::CylinderY, toward_neg_x(0.0, 0.45)),
        10.0 - 0.217_944_95,
    );
}

#[test]
fn a_ray_along_a_cylinder_axis_but_off_its_radius_misses() {
    // Parallel to the axis: no quadratic to solve, only "inside the disc?"
    assert!(unit(ShapeKind::CylinderX, toward_neg_x(0.6, 0.0)).is_none());
    assert_close(unit(ShapeKind::CylinderX, toward_neg_x(0.4, 0.0)), 9.5);
}

// --- Wedge ----------------------------------------------------------------

#[test]
fn a_wedge_is_hit_under_its_slope() {
    // Coming down at z = -0.45, near the front edge, the slope `y = z` is met
    // at y = -0.45: 0.95 studs below where the box's top face would be.
    let down = toward_neg_y(0.0, -0.45);
    assert_close(box_only(down), 9.5);
    assert_close(unit(ShapeKind::Wedge, down), 10.45);
    // Coming from the front, the slope is met where it reaches y = 0.45.
    let from_front = Ray::new(Vec3::new(0.0, 0.45, -10.0), Vec3::Z);
    assert_close(unit(ShapeKind::Wedge, from_front), 10.45);
}

#[test]
fn a_wedge_is_missed_in_the_air_above_its_slope() {
    // Above the slope, across the whole width: nothing there but box.
    let across = toward_neg_x(0.45, -0.45);
    assert!(box_only(across).is_some());
    assert!(unit(ShapeKind::Wedge, across).is_none());
}

#[test]
fn a_wedge_keeps_its_vertical_back_face() {
    // The tall face stands at +Z, so from behind it is met like a box.
    assert_close(unit(ShapeKind::Wedge, toward_neg_z(0.0, 0.45)), 9.5);
}

// --- CornerWedge ----------------------------------------------------------

#[test]
fn a_corner_wedge_is_hit_under_both_slopes() {
    // Far from the apex (+X, -Z corner), both slopes have dropped almost to
    // the floor.
    assert_close(
        unit(ShapeKind::CornerWedge, toward_neg_y(-0.45, 0.45)),
        10.45,
    );
    // Under the apex, both are nearly at the top.
    assert_close(
        unit(ShapeKind::CornerWedge, toward_neg_y(0.45, -0.45)),
        9.55,
    );
    // Along one edge only the `y = -z` slope is low.
    assert_close(
        unit(ShapeKind::CornerWedge, toward_neg_y(0.45, 0.45)),
        10.45,
    );
}

#[test]
fn a_corner_wedge_is_missed_at_its_open_corners() {
    let across = toward_neg_x(0.45, 0.45);
    assert!(box_only(across).is_some());
    assert!(unit(ShapeKind::CornerWedge, across).is_none());
}

// --- Arbitrary CFrame -----------------------------------------------------

#[test]
fn a_rotated_wedge_is_hit_on_its_slope_later_than_its_box() {
    let model = arbitrary_model();
    // A point on the slope, approached from the air above it along the
    // slope's own (local) normal: the box is entered first, where the ray
    // crosses its top face, and the slope only at the point itself.
    let on_slope = model.transform_point3(Vec3::new(0.2, 0.1, 0.1));
    let start = model.transform_point3(Vec3::new(0.2, 3.1, -2.9));
    let ray = Ray::new(start, on_slope - start);

    assert_close(
        hit(ShapeKind::Wedge, model, ray),
        (on_slope - start).length(),
    );
    let box_face = model.transform_point3(Vec3::new(0.2, 0.5, -0.3));
    assert_close(hit(ShapeKind::Box, model, ray), (box_face - start).length());
}

#[test]
fn a_rotated_wedge_is_missed_across_the_gap_above_its_slope() {
    let model = arbitrary_model();
    // Along the part's own X axis at a height the slope never reaches.
    let start = model.transform_point3(Vec3::new(5.0, 0.4, -0.4));
    let ray = Ray::new(start, model.transform_vector3(Vec3::NEG_X));

    assert!(hit(ShapeKind::Box, model, ray).is_some());
    assert!(hit(ShapeKind::Wedge, model, ray).is_none());
}

#[test]
fn a_flattened_solid_is_not_pickable() {
    let model = Mat4::from_scale(Vec3::new(4.0, 0.0, 4.0));
    for kind in [ShapeKind::Ball, ShapeKind::CylinderX, ShapeKind::Wedge] {
        assert!(hit(kind, model, toward_neg_y(0.0, 0.0)).is_none());
    }
}

// --- Against the drawn meshes ---------------------------------------------

/// Every solid's analytic test, checked against the triangles `shapes`
/// builds for the GPU — the definition of what is on screen — across a grid
/// of rays from four directions, under the identity and under an arbitrary
/// transform. This is what pins the slope planes and the cylinder axes to the
/// geometry rather than to a reading of it.
#[test]
fn every_solid_agrees_with_the_mesh_it_is_drawn_as() {
    let solids: [(ShapeKind, MeshData, bool); 6] = [
        (ShapeKind::Box, shapes::block(), false),
        (ShapeKind::Wedge, shapes::wedge(), false),
        (ShapeKind::CornerWedge, shapes::corner_wedge(), false),
        (ShapeKind::Ball, shapes::sphere(), true),
        (ShapeKind::CylinderX, shapes::cylinder_x(), true),
        (ShapeKind::CylinderY, shapes::cylinder_y(), true),
    ];
    for (kind, data, curved) in &solids {
        let mesh = as_file_mesh(data);
        for model in [Mat4::IDENTITY, arbitrary_model()] {
            let mut compared = 0;
            for ray in grid_of_rays(model) {
                if *curved && grazes(*kind, model, ray) {
                    continue;
                }
                let analytic = hit(*kind, model, ray);
                let drawn = mesh::hit(&mesh, model, ray);
                // The polygonal curve sits a hair inside the true one; a flat
                // face is the same plane in both.
                let tolerance = if *curved { 0.1 } else { 1e-3 };
                match (analytic, drawn) {
                    (None, None) => {}
                    (Some(a), Some(d)) => assert!(
                        (a - d).abs() < tolerance,
                        "{kind:?}: analytic {a} vs drawn {d} for {ray:?}"
                    ),
                    _ => panic!("{kind:?}: analytic {analytic:?} vs drawn {drawn:?} for {ray:?}"),
                }
                compared += 1;
            }
            assert!(compared > 200, "{kind:?}: too few rays compared");
        }
    }
}

/// Parallel rays through a plane of offsets, from -Z, -X, -Y and one oblique
/// direction, all expressed in the solid's local space and then carried
/// through `model`: what covers the solid however it is placed.
fn grid_of_rays(model: Mat4) -> Vec<Ray> {
    let mut rays = Vec::new();
    // Two step sizes that never produce `a == b` or `a == -b`: those rays
    // would run exactly inside a wedge's slope plane, where a plane test and
    // a triangle test are both entitled to their own answer.
    let along = (-5..=5).map(|step| step as f32 * 0.14);
    let across = (-5..=5).map(|step| step as f32 * 0.13 + 0.005);
    for a in along {
        for b in across.clone() {
            let locals = [
                (Vec3::new(a, b, 10.0), Vec3::NEG_Z),
                (Vec3::new(10.0, a, b), Vec3::NEG_X),
                (Vec3::new(a, 10.0, b), Vec3::NEG_Y),
                // Reaches z = 0 at (a, b, 0).
                (
                    Vec3::new(a + 3.0, b - 2.0, 10.0),
                    Vec3::new(-0.3, 0.2, -1.0),
                ),
            ];
            for (origin, direction) in locals {
                rays.push(Ray::new(
                    model.transform_point3(origin),
                    model.transform_vector3(direction),
                ));
            }
        }
    }
    rays
}

/// Whether `ray` passes within a few hundredths of a curved solid's
/// silhouette, where a 24-gon and a circle legitimately disagree on whether
/// there is anything to hit.
fn grazes(kind: ShapeKind, model: Mat4, ray: Ray) -> bool {
    let local = Local::of(model, ray).expect("test models are invertible");
    let radial = |mut vector: Vec3| {
        match kind {
            ShapeKind::CylinderX => vector.x = 0.0,
            ShapeKind::CylinderY => vector.y = 0.0,
            _ => {}
        }
        vector
    };
    let (origin, direction) = (radial(local.origin), radial(local.direction));
    let along = direction.length_squared();
    let closest = if along < f32::EPSILON {
        origin.length()
    } else {
        (origin - direction * (origin.dot(direction) / along)).length()
    };
    (closest - RADIUS).abs() < 0.03
}

fn as_file_mesh(data: &MeshData) -> rbx_mesh::Mesh {
    rbx_mesh::Mesh {
        version: (4, 1),
        vertices: data
            .positions
            .iter()
            .map(|&position| rbx_mesh::Vertex {
                position,
                normal: [0.0; 3],
                uv: [0.0; 2],
                color: [255; 4],
            })
            .collect(),
        indices: data.indices.clone(),
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-0.5; 3],
            max: [0.5; 3],
        },
    }
}
