use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::Ray;

use super::{bounds, count_scripts, face_of, land, shift, subtree, Aim, OPEN_AIR_DISTANCE};

fn vec3(x: f32, y: f32, z: f32) -> rbx_dom::Vector3Data {
    rbx_dom::Vector3Data { x, y, z }
}

fn cframe(x: f32, y: f32, z: f32) -> rbx_dom::CFrameData {
    rbx_dom::CFrameData {
        position: vec3(x, y, z),
        rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    }
}

fn add_part(dom: &mut WeakDom, parent: Option<Ref>, at: (f32, f32, f32)) -> Ref {
    let part = dom.new_instance("Part", "Part", parent);
    dom.set_property(part, "CFrame", Variant::CFrame(cframe(at.0, at.1, at.2)))
        .unwrap();
    // The DOM keeps `BasePart.Size` under the binary format's lowercase name.
    dom.set_property(part, "size", Variant::Vector3(vec3(4., 2., 4.)))
        .unwrap();
    part
}

fn position(dom: &WeakDom, r: Ref) -> [f32; 3] {
    match dom.get(r).and_then(|i| i.properties().get("CFrame")) {
        Some(Variant::CFrame(c)) => [c.position.x, c.position.y, c.position.z],
        other => panic!("no CFrame: {other:?}"),
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn down_from(origin: Vec3) -> Ray {
    Ray::new(origin, Vec3::new(0., -1., 0.))
}

#[test]
fn a_part_lands_on_the_hit_point() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    let aim = Aim {
        ray: Some(down_from(Vec3::new(0., 50., 0.))),
        part: None,
        distance: Some(20.),
        normal: None,
    };
    land(&mut dom, &[part], &aim);
    let p = position(&dom, part);
    assert!(near(p[0], 0.) && near(p[1], 31.) && near(p[2], 0.), "{p:?}");
}

#[test]
fn a_part_in_open_air_lands_64_studs_along_the_ray() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    let aim = Aim {
        ray: Some(Ray::new(Vec3::new(0., 5., 0.), Vec3::new(0., 0., -1.))),
        part: None,
        distance: None,
        normal: None,
    };
    land(&mut dom, &[part], &aim);
    let p = position(&dom, part);
    assert!(
        near(p[0], 0.) && near(p[1], 5.) && near(p[2], -OPEN_AIR_DISTANCE),
        "{p:?}"
    );
}

#[test]
fn a_model_moves_its_parts_and_pivot_together() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let a = add_part(&mut dom, Some(model), (0., 3., 0.));
    let b = add_part(&mut dom, Some(model), (10., 3., 0.));
    dom.set_property(
        model,
        "WorldPivotData",
        Variant::OptionalCFrame(Some(cframe(5., 3., 0.))),
    )
    .unwrap();
    let aim = Aim {
        ray: Some(down_from(Vec3::new(0., 20., 0.))),
        part: None,
        distance: Some(10.),
        normal: None,
    };
    land(&mut dom, &[model], &aim);
    let (pa, pb) = (position(&dom, a), position(&dom, b));
    assert!(
        near(pa[0], -5.) && near(pa[1], 11.) && near(pa[2], 0.),
        "{pa:?}"
    );
    assert!(
        near(pb[0], 5.) && near(pb[1], 11.) && near(pb[2], 0.),
        "{pb:?}"
    );
    assert!(near(pb[0] - pa[0], 10.));
    match dom
        .get(model)
        .and_then(|i| i.properties().get("WorldPivotData"))
    {
        Some(Variant::OptionalCFrame(Some(c))) => {
            let p = c.position;
            assert!(near(p.x, 0.) && near(p.y, 11.) && near(p.z, 0.));
        }
        other => panic!("pivot lost: {other:?}"),
    }
}

#[test]
fn scripts_are_counted_at_any_depth() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    dom.new_instance("Script", "S", Some(model));
    let folder = dom.new_instance("Folder", "F", Some(model));
    dom.new_instance("LocalScript", "L", Some(folder));
    let deep = dom.new_instance("Folder", "D", Some(folder));
    dom.new_instance("ModuleScript", "M", Some(deep));
    dom.new_instance("Part", "P", Some(deep));
    dom.new_instance("StringValue", "V", Some(deep));
    assert_eq!(count_scripts(&dom, &[model]), 3);
}

#[test]
fn nothing_moves_without_a_ray() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    land(&mut dom, &[part], &Aim::default());
    let p = position(&dom, part);
    assert!(near(p[0], 5.) && near(p[1], 10.) && near(p[2], 7.), "{p:?}");
}

#[test]
fn a_subtree_holds_the_root_and_all_descendants() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let child = dom.new_instance("Folder", "F", Some(model));
    let leaf = dom.new_instance("Part", "P", Some(child));
    let all = subtree(&dom, model);
    assert_eq!(all.len(), 3);
    assert!(all.contains(&model) && all.contains(&child) && all.contains(&leaf));
}

#[test]
fn bounds_wrap_a_scaled_box_around_its_centre() {
    let model = glam::Mat4::from_scale_rotation_translation(
        Vec3::new(4., 2., 6.),
        glam::Quat::IDENTITY,
        Vec3::new(1., 2., 3.),
    );
    let (min, max) = bounds(model);
    assert!(near(min.x, -1.) && near(min.y, 1.) && near(min.z, 0.));
    assert!(near(max.x, 3.) && near(max.y, 3.) && near(max.z, 6.));
}

#[test]
fn shifting_a_cframe_keeps_its_rotation() {
    let moved = shift(cframe(1., 2., 3.), Vec3::new(1., -2., 0.5));
    assert!(
        near(moved.position.x, 2.) && near(moved.position.y, 0.) && near(moved.position.z, 3.5)
    );
    assert_eq!(moved.rotation, [1., 0., 0., 0., 1., 0., 0., 0., 1.]);
}

#[test]
fn a_decal_takes_the_face_the_view_hits() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (0., 0., 0.));
    assert_eq!(face_of(&dom, part, Vec3::Y), Some(1), "Top");
    assert_eq!(face_of(&dom, part, Vec3::NEG_Z), Some(5), "Front");
    assert_eq!(face_of(&dom, part, Vec3::X), Some(0), "Right");
    // A part turned a quarter about Y: its Right face looks down -Z.
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: vec3(0., 0., 0.),
            rotation: [0., 0., 1., 0., 1., 0., -1., 0., 0.],
        }),
    )
    .unwrap();
    assert_eq!(face_of(&dom, part, Vec3::NEG_Z), Some(0), "turned Right");
}
