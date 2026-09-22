use glam::{Mat4, Vec2, Vec3};
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::{Meshes, PartSurface, Ray};

use super::*;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

/// A part of `class` at the origin, `size` big, `shape` set if given.
fn part(class: &str, size: Vec3, shape: Option<u32>) -> PartSurface {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance(class, "Part", Some(workspace));
    let zero = Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let rotation = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: zero,
            rotation,
        }),
    )
    .unwrap();
    let size = Vector3Data {
        x: size.x,
        y: size.y,
        z: size.z,
    };
    dom.set_property(part, "size", Variant::Vector3(size))
        .unwrap();
    if let Some(shape) = shape {
        dom.set_property(part, "shape", Variant::Enum(shape))
            .unwrap();
    }
    PartSurface::read(
        &dom,
        &ReflectionDatabase::embedded(),
        &Meshes::default(),
        part,
    )
    .unwrap()
}

/// The frame under a ray straight down at (x, z).
fn from_above(surface: &PartSurface, x: f32, z: f32, grid: f32) -> SurfaceFrame {
    let ray = Ray::new(Vec3::new(x, 50.0, z), Vec3::NEG_Y);
    let (distance, normal) = surface.raycast(ray).expect("the ray meets the part");
    target_frame(surface, ray.at(distance), normal, grid).expect("a frame")
}

#[test]
fn a_box_is_its_face_cornered_nearest_the_cursor() {
    let block = part("Part", Vec3::new(8.0, 1.0, 4.0), None);
    let frame = from_above(&block, 3.0, 1.5, 1.0);
    assert_eq!(frame.kind, TargetKind::Polygon);
    assert!(close(frame.corner, Vec3::new(4.0, 0.5, 2.0)));
    assert!(frame.part.is_none());
}

#[test]
fn a_wedges_slope_is_a_face_of_its_own_edges() {
    // 2 high, 4 deep: the slope climbs from the front-bottom edge (z = -2)
    // to the back-top one (z = 2).
    let wedge = part("WedgePart", Vec3::new(4.0, 2.0, 4.0), None);
    let frame = from_above(&wedge, 1.6, 1.8, 1.0);
    assert!(
        close(frame.y, Vec3::new(0.0, 4.0, -2.0).normalize()),
        "{}",
        frame.y
    );
    // Nearest the top edge's +X end.
    assert!(
        close(frame.corner, Vec3::new(2.0, 1.0, 2.0)),
        "{}",
        frame.corner
    );
    // Across the slope its size is the hypotenuse, along it the width.
    let hypotenuse = (4.0f32 * 4.0 + 2.0 * 2.0).sqrt();
    let sizes = [frame.size.x, frame.size.y];
    assert!(
        sizes.iter().any(|s| (s - hypotenuse).abs() < 1e-3),
        "{sizes:?}"
    );
    assert!(sizes.iter().any(|s| (s - 4.0).abs() < 1e-3), "{sizes:?}");
}

#[test]
fn a_ball_is_the_snapped_point_on_it_facing_out() {
    let ball = part("Part", Vec3::splat(4.0), Some(0));
    let frame = from_above(&ball, 1.5, 0.9, 1.0);
    assert_eq!(frame.kind, TargetKind::Sphere);
    assert!(matches!(frame.part, Some((Solid::Ball, _))));
    // On the ball, facing out from its centre.
    assert!(
        (frame.corner.length() - 2.0).abs() < 1e-3,
        "{}",
        frame.corner
    );
    assert!(close(frame.y, frame.corner.normalize()));
    // The height up the ball rounds to whole studs.
    assert!(
        (frame.corner.y - frame.corner.y.round()).abs() < 1e-3,
        "{}",
        frame.corner
    );
}

#[test]
fn near_its_pole_a_ball_is_a_point_at_the_pole() {
    let ball = part("Part", Vec3::splat(4.0), Some(0));
    let frame = from_above(&ball, 0.2, 0.1, 1.0);
    assert_eq!(frame.kind, TargetKind::Polygon);
    assert!(
        close(frame.corner, Vec3::new(0.0, 2.0, 0.0)),
        "{}",
        frame.corner
    );
    assert_eq!(frame.size, Vec2::ZERO);
}

#[test]
fn a_cylinders_side_is_measured_from_its_nearer_end() {
    let cylinder = part("Part", Vec3::new(8.0, 2.0, 2.0), Some(2));
    let frame = from_above(&cylinder, 2.6, 0.1, 1.0);
    assert_eq!(frame.kind, TargetKind::Cylinder);
    // The top generator, at the +X end's rim.
    assert!(
        close(frame.corner, Vec3::new(4.0, 1.0, 0.0)),
        "{}",
        frame.corner
    );
    assert!(close(frame.y, Vec3::Y));
    assert!(frame.z.dot(Vec3::X).abs() > 0.999);
}

#[test]
fn a_cylinders_cap_is_an_r_square_from_its_nearest_anchor() {
    let cylinder = part("Part", Vec3::new(8.0, 4.0, 4.0), Some(2));
    let ray = Ray::new(Vec3::new(20.0, 1.5, -0.3), Vec3::NEG_X);
    let (distance, normal) = cylinder.raycast(ray).unwrap();
    let frame = target_frame(&cylinder, ray.at(distance), normal, 1.0).unwrap();
    assert_eq!(frame.kind, TargetKind::Polygon);
    // Up past half the radius, across not: the rim point on +Y.
    assert!(
        close(frame.corner, Vec3::new(4.0, 2.0, 0.0)),
        "{}",
        frame.corner
    );
    assert_eq!(frame.size, Vec2::splat(2.0));
}

#[test]
fn a_meshs_face_is_measured_from_the_edge_the_probes_find() {
    // No mesh downloaded: the part stands as its box, and the probes find the
    // box's edges.
    let mesh = part("MeshPart", Vec3::new(8.0, 2.0, 4.0), None);
    let frame = from_above(&mesh, 3.1, 0.4, 1.0);
    assert_eq!(frame.kind, TargetKind::Polygon);
    // The +X edge is 0.9 away, the +Z edge 1.6: the corner is at the +X
    // edge's end nearer the hit.
    assert!(
        close(frame.corner, Vec3::new(4.0, 1.0, 2.0)),
        "{}",
        frame.corner
    );
    assert!(frame.z.dot(Vec3::Z).abs() > 0.999, "{}", frame.z);
}

#[test]
fn a_surface_with_no_edge_snaps_to_its_parts_lattice() {
    let model = Mat4::from_scale(Vec3::splat(4.0));
    let frame = round(model, Vec3::new(0.7, 2.0, -1.2), Vec3::Y, 1.0).unwrap();
    assert_eq!(frame.kind, TargetKind::Round);
    assert!(
        close(frame.corner, Vec3::new(1.0, 2.0, -1.0)),
        "{}",
        frame.corner
    );
}
