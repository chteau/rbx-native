use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_ugc::{
    Affine, AssetType, BodyScale, Cage, CageKind, Finding, Geometry, Item, Part, Rule, Severity,
};

use super::{cframe_matrix, drawable, segments, Place};
use crate::shell::ugc::Collected;
use crate::tokens::linear;

const CUBE: [[u32; 3]; 12] = [
    [0, 2, 1],
    [0, 3, 2],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [1, 2, 6],
    [1, 6, 5],
    [2, 3, 7],
    [2, 7, 6],
    [3, 0, 4],
    [3, 4, 7],
];

fn cube(triangles: &[[u32; 3]]) -> Geometry {
    Geometry {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
        triangles: triangles.to_vec(),
        uvs: vec![[0.5, 0.5]; 8],
        in_studs: true,
        ..Default::default()
    }
}

fn place_at(dom: &mut WeakDom, part: Ref, x: f32) {
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y: 0.0, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
}

fn finding(rule: Rule, subject: &str) -> Finding {
    Finding {
        rule,
        severity: Severity::Warning,
        subject: subject.into(),
        measured: None,
        limit: None,
        message: String::new(),
    }
}

fn collected(parts: Vec<Part>, cages: Vec<Cage>, places: Vec<Place>) -> Collected {
    Collected {
        item: Item {
            name: "Cap".into(),
            asset_type: AssetType::Hat,
            scale: BodyScale::Normal,
            parts,
            textures: vec![],
            cages,
        },
        emissive: false,
        notes: vec![],
        from_file: false,
        head: None,
        places,
    }
}

fn holed_hat(dom: &mut WeakDom) -> (Ref, Collected) {
    let handle = dom.new_instance("MeshPart", "Handle", None);
    place_at(dom, handle, 10.0);
    let part = Part {
        name: "Handle".into(),
        geometry: Some(cube(&CUBE[..11])),
        cframe: rbx_ugc::IDENTITY,
        attachments: vec![],
    };
    let place = Place {
        name: "Handle".into(),
        node: handle,
        local: Mat4::IDENTITY,
    };
    (handle, collected(vec![part], vec![], vec![place]))
}

#[test]
fn only_checks_with_vertices_to_point_at_are_drawn() {
    for rule in [
        Rule::Watertight,
        Rule::CageUvs,
        Rule::DegenerateTriangles,
        Rule::MeshOutsideCage,
        Rule::CageDistance,
    ] {
        assert!(drawable(rule));
    }
    for rule in [
        Rule::ItemSize,
        Rule::Attachment,
        Rule::AccessoryTriangles,
        Rule::CageTopology,
    ] {
        assert!(!drawable(rule));
    }
}

#[test]
fn a_hole_is_drawn_as_studio_draws_open_edges() {
    let mut dom = WeakDom::new();
    let (_, item) = holed_hat(&mut dom);
    let out = segments(&dom, &item, &finding(Rule::Watertight, "Handle"));
    let dots: Vec<_> = out.iter().filter(|s| s.from == s.to).collect();
    assert_eq!(dots.len(), 3);
    assert!(dots
        .iter()
        .all(|d| d.on_top && d.color == linear(gpui_kit::rgb(0xff4040))));
    let red: Vec<_> = out
        .iter()
        .filter(|s| s.from != s.to && s.color == linear(gpui_kit::rgb(0x7f2020)))
        .collect();
    // Every edge meeting a corner of the hole, diagonals included.
    assert_eq!(red.len(), 10);
    assert!(out
        .iter()
        .filter(|s| s.from != s.to)
        .all(|s| !s.on_top && s.width == 2.0));
    // The vertex at the origin of the mesh sits 10 studs along X with its part.
    assert!(dots.iter().any(|d| d.from == Vec3::new(10.0, 1.0, 0.0)));
}

#[test]
fn the_drawing_follows_the_part_and_goes_with_it() {
    let mut dom = WeakDom::new();
    let (handle, item) = holed_hat(&mut dom);
    let check = finding(Rule::Watertight, "Handle");
    place_at(&mut dom, handle, -4.0);
    let moved = segments(&dom, &item, &check);
    assert!(moved
        .iter()
        .any(|d| d.from == d.to && d.from == Vec3::new(-4.0, 1.0, 0.0)));
    dom.remove(handle);
    assert!(segments(&dom, &item, &check).is_empty());
}

#[test]
fn a_cage_with_a_moved_uv_draws_the_whole_cage_and_marks_the_vertex() {
    let mut dom = WeakDom::new();
    let handle = dom.new_instance("MeshPart", "Handle", None);
    place_at(&mut dom, handle, 0.0);
    let inner = cube(&CUBE);
    let mut outer = inner.clone();
    outer.uvs[6] = [0.9, 0.1];
    let cage = |kind, name: &str, geometry: Geometry, template| Cage {
        kind,
        name: name.into(),
        geometry,
        template,
        to_mesh: rbx_ugc::IDENTITY,
    };
    let lifted = Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0));
    let item = collected(
        vec![],
        vec![
            cage(CageKind::Inner, "Cap_InnerCage", inner.clone(), None),
            cage(CageKind::Outer, "Cap_OuterCage", outer, Some(inner)),
        ],
        vec![Place {
            name: "Cap_OuterCage".into(),
            node: handle,
            local: lifted,
        }],
    );
    let out = segments(&dom, &item, &finding(Rule::CageUvs, "Cap_OuterCage"));
    let lines = out.iter().filter(|s| s.from != s.to).count();
    assert_eq!(lines, 18, "every edge of the cube cage");
    let dots: Vec<_> = out.iter().filter(|s| s.from == s.to).collect();
    assert_eq!(dots.len(), 1);
    assert_eq!(dots[0].from, Vec3::new(1.0, 3.0, 1.0));
    let red = out
        .iter()
        .filter(|s| s.from != s.to && s.color == linear(gpui_kit::rgb(0x7f2020)))
        .count();
    assert_eq!(red, 5, "the edges meeting the moved vertex");
}

#[test]
fn findings_without_a_place_or_a_drawing_draw_nothing() {
    let mut dom = WeakDom::new();
    let (_, item) = holed_hat(&mut dom);
    assert!(segments(&dom, &item, &finding(Rule::ItemSize, "Handle")).is_empty());
    assert!(segments(&dom, &item, &finding(Rule::Watertight, "Nope")).is_empty());
}

#[test]
fn a_cframe_turns_like_roblox_rotates() {
    // A quarter turn about Y: Roblox's CFrame.Angles(0, math.pi / 2, 0).
    let c = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0];
    let p = cframe_matrix(&c).transform_point3(Vec3::X);
    assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
}

fn dots(out: &[rbx_viewer::Segment]) -> Vec<Vec3> {
    out.iter()
        .filter(|s| s.from == s.to)
        .map(|s| s.from)
        .collect()
}

#[test]
fn a_collapsed_triangle_marks_its_corners() {
    let mut dom = WeakDom::new();
    let (_, mut item) = holed_hat(&mut dom);
    let geometry = item.item.parts[0].geometry.as_mut().unwrap();
    geometry.triangles = CUBE.to_vec();
    // A triangle with a corner halfway along the 0-1 edge.
    geometry.positions.push([0.5, 0.0, 0.0]);
    geometry.triangles.push([0, 1, 8]);
    let out = segments(&dom, &item, &finding(Rule::DegenerateTriangles, "Handle"));
    let mut at = dots(&out);
    at.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        at,
        vec![
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.5, 0.0, 0.0),
            Vec3::new(11.0, 0.0, 0.0)
        ]
    );
}

fn jacket(dom: &mut WeakDom, cage_scale: f32) -> Collected {
    let handle = dom.new_instance("MeshPart", "Handle", None);
    place_at(dom, handle, 0.0);
    let mut outer = cube(&CUBE);
    for p in &mut outer.positions {
        *p = p.map(|c| 0.5 + (c - 0.5) * cage_scale);
    }
    let cage = |kind, name: &str, geometry: Geometry, to_mesh: Affine| Cage {
        kind,
        name: name.into(),
        geometry,
        template: None,
        to_mesh,
    };
    // The outer cage file sits 5 studs up; its origin brings it back.
    let mut lifted = outer.clone();
    for p in &mut lifted.positions {
        p[1] += 5.0;
    }
    let mut back = rbx_ugc::IDENTITY;
    back[1] = -5.0;
    let place = |name: &str, local| Place {
        name: name.into(),
        node: handle,
        local,
    };
    let mut item = collected(
        vec![Part {
            name: "Handle".into(),
            geometry: Some(cube(&CUBE)),
            cframe: rbx_ugc::IDENTITY,
            attachments: vec![],
        }],
        vec![
            cage(
                CageKind::Inner,
                "Cap_InnerCage",
                cube(&CUBE),
                rbx_ugc::IDENTITY,
            ),
            cage(CageKind::Outer, "Cap_OuterCage", lifted, back),
        ],
        vec![
            place("Handle", Mat4::IDENTITY),
            place(
                "Cap_OuterCage",
                Mat4::from_translation(Vec3::new(0.0, -5.0, 0.0)),
            ),
        ],
    );
    item.item.asset_type = AssetType::Jacket;
    item
}

#[test]
fn a_mesh_outside_its_cage_marks_the_vertices_poking_out() {
    let mut dom = WeakDom::new();
    let item = jacket(&mut dom, 0.5);
    let out = segments(&dom, &item, &finding(Rule::MeshOutsideCage, "Handle"));
    assert_eq!(dots(&out).len(), 8, "every corner of the handle");
    assert!(dots(&out).contains(&Vec3::new(1.0, 1.0, 1.0)));
    let inside = jacket(&mut dom, 1.5);
    assert!(dots(&segments(
        &dom,
        &inside,
        &finding(Rule::MeshOutsideCage, "Handle")
    ))
    .is_empty());
}

#[test]
fn a_cage_too_far_from_the_mesh_draws_the_whole_cage() {
    let mut dom = WeakDom::new();
    let item = jacket(&mut dom, 3.0);
    let out = segments(&dom, &item, &finding(Rule::CageDistance, "Cap_OuterCage"));
    assert_eq!(out.iter().filter(|s| s.from != s.to).count(), 18);
    // The cage corners sit a stud out on each axis, drawn where the place
    // puts the cage.
    assert!(dots(&out).contains(&Vec3::new(2.0, 2.0, 2.0)));
    assert_eq!(dots(&out).len(), 8);
}
