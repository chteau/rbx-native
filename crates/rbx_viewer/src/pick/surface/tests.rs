use glam::Vec3;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;

/// A part of `class` at the origin, `size` big, with `shape` set if given.
fn part(class: &str, size: Vec3, shape: Option<u32>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance(class, "Part", Some(workspace));
    let rotation = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let origin = Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: origin,
            rotation,
        }),
    )
    .unwrap();
    dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: size.x,
            y: size.y,
            z: size.z,
        }),
    )
    .unwrap();
    if let Some(shape) = shape {
        dom.set_property(part, "shape", Variant::Enum(shape))
            .unwrap();
    }
    (dom, part)
}

fn surface(class: &str, size: Vec3, shape: Option<u32>) -> PartSurface {
    let (dom, part) = part(class, size, shape);
    PartSurface::read(
        &dom,
        &ReflectionDatabase::embedded(),
        &Meshes::default(),
        part,
    )
    .expect("a part with a placement")
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn the_solid_follows_the_class_and_the_shape() {
    assert_eq!(surface("Part", Vec3::ONE, None).solid, Solid::Box);
    assert_eq!(surface("Part", Vec3::ONE, Some(0)).solid, Solid::Ball);
    assert_eq!(surface("Part", Vec3::ONE, Some(2)).solid, Solid::Cylinder);
    assert_eq!(surface("WedgePart", Vec3::ONE, None).solid, Solid::Wedge);
    assert_eq!(
        surface("CornerWedgePart", Vec3::ONE, None).solid,
        Solid::CornerWedge
    );
    assert_eq!(surface("MeshPart", Vec3::ONE, None).solid, Solid::Mesh);
    assert_eq!(surface("TrussPart", Vec3::ONE, None).solid, Solid::Box);
}

#[test]
fn a_box_face_answers_with_its_own_normal() {
    let block = surface("Part", Vec3::new(4.0, 2.0, 2.0), None);
    let (distance, normal) = block
        .raycast(Ray::new(Vec3::new(0.5, 10.0, 0.3), Vec3::NEG_Y))
        .expect("straight down onto the top");
    assert!((distance - 9.0).abs() < 1e-4);
    assert!(close(normal, Vec3::Y));
}

#[test]
fn a_wedges_slope_faces_up_and_forward_by_its_proportions() {
    // 2 high, 4 deep: the slope rises 2 over 4, so its normal leans forward
    // by atan(4/2) from the vertical.
    let wedge = surface("WedgePart", Vec3::new(2.0, 2.0, 4.0), None);
    let (_, normal) = wedge
        .raycast(Ray::new(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y))
        .expect("down onto the slope");
    assert!(
        close(normal, Vec3::new(0.0, 4.0, -2.0).normalize()),
        "{normal}"
    );
}

#[test]
fn a_ball_faces_out_from_its_centre() {
    let ball = surface("Part", Vec3::splat(2.0), Some(0));
    let direction = Vec3::new(-1.0, -1.0, 0.0).normalize();
    let (_, normal) = ball
        .raycast(Ray::new(-direction * 10.0, direction))
        .expect("aimed at the centre");
    assert!(close(normal, -direction), "{normal}");
}

#[test]
fn a_cylinder_has_caps_and_a_round_side() {
    let cylinder = surface("Part", Vec3::new(6.0, 2.0, 2.0), Some(2));
    let (_, cap) = cylinder
        .raycast(Ray::new(Vec3::new(10.0, 0.2, 0.1), Vec3::NEG_X))
        .expect("into the +X cap");
    assert!(close(cap, Vec3::X), "{cap}");
    let (_, side) = cylinder
        .raycast(Ray::new(Vec3::new(1.0, 10.0, 0.0), Vec3::NEG_Y))
        .expect("onto the side");
    assert!(close(side, Vec3::Y), "{side}");
}
