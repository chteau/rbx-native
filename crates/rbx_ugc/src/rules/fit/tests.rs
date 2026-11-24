use super::super::distance::tests::cube;
use super::*;
use crate::{AssetType, BodyScale, Part, IDENTITY};

/// A box whose corners have UVs of their own, as a cage's do.
fn boxed(lo: f32, hi: f32) -> Geometry {
    let (positions, triangles) = cube(lo, hi);
    Geometry {
        uvs: (0..8).map(|i| [i as f32 / 8.0, 0.5]).collect(),
        positions,
        triangles,
        in_studs: true,
        ..Default::default()
    }
}

fn jacket(mesh: Geometry, outer: Geometry, to_mesh: Affine) -> Item {
    let cage = |kind, name: &str, geometry| Cage {
        kind,
        name: name.into(),
        geometry,
        template: None,
        to_mesh,
    };
    Item {
        name: "Coat".into(),
        asset_type: AssetType::Jacket,
        scale: BodyScale::Normal,
        parts: vec![Part {
            name: "Handle".into(),
            geometry: Some(mesh),
            cframe: IDENTITY,
            attachments: vec![],
        }],
        textures: vec![],
        cages: vec![
            cage(CageKind::Inner, "Coat_InnerCage", boxed(-0.9, 0.9)),
            cage(CageKind::Outer, "Coat_OuterCage", outer),
        ],
    }
}

fn run(item: &Item) -> Vec<Finding> {
    let mut out = Vec::new();
    fit(item, &mut out);
    out
}

/// Two boxes in one mesh: 8 of its 16 vertices sit at 3..4 on x.
fn two_boxes() -> Geometry {
    let mut mesh = boxed(-1.0, 1.0);
    let far = boxed(3.0, 4.0);
    mesh.triangles
        .extend(far.triangles.iter().map(|t| t.map(|i| i + 8)));
    mesh.positions.extend(far.positions);
    mesh
}

fn messages(item: &Item) -> Vec<(Severity, String)> {
    run(item)
        .into_iter()
        .map(|f| (f.severity, f.message))
        .collect()
}

#[test]
fn a_snug_cage_passes() {
    let item = jacket(boxed(-1.0, 1.0), boxed(-1.2, 1.2), IDENTITY);
    assert!(mesh_outside_cage(&item).is_empty());
    let d = cage_mesh_distances(&item);
    assert_eq!(d.len(), 8);
    assert!(d.iter().all(|(_, d)| (d - 0.2 * 3f32.sqrt()).abs() < 1e-4));
    assert!(run(&item).is_empty());
}

#[test]
fn rigid_items_are_not_measured() {
    let mut item = jacket(boxed(-1.0, 1.0), boxed(-0.5, 0.5), IDENTITY);
    item.asset_type = AssetType::Hat;
    assert!(run(&item).is_empty());
    assert!(mesh_outside_cage(&item).is_empty());
}

#[test]
fn half_the_mesh_outside_is_only_a_warning() {
    let item = jacket(two_boxes(), boxed(-1.2, 1.2), IDENTITY);
    assert_eq!(mesh_outside_cage(&item), (8..16).collect::<Vec<_>>());
    let outside: Vec<_> = messages(&item)
        .into_iter()
        .filter(|(_, m)| m.contains("mesh vertices"))
        .collect();
    assert_eq!(
        outside,
        vec![(
            Severity::Warning,
            "50.00% of mesh vertices are outside the outer cage. Please adjust the outer cage to fully contain the render mesh.".to_string()
        )]
    );
}

#[test]
fn a_mesh_mostly_outside_fails_with_studios_messages() {
    let item = jacket(boxed(-1.0, 1.0), boxed(-0.5, 0.5), IDENTITY);
    let f = run(&item);
    let outside: Vec<_> = f
        .iter()
        .filter(|f| f.rule == Rule::MeshOutsideCage)
        .collect();
    assert_eq!(outside.len(), 2);
    assert_eq!(outside[0].severity, Severity::Error);
    assert_eq!(outside[0].subject, "Handle");
    assert_eq!(outside[0].measured, Some(Value::Text("100.00%".into())));
    assert_eq!(
        outside[0].message,
        "100.00% of mesh vertices for the accessory are outside its outer cage. Make adjustments to have more of the accessory placed between the cages."
    );
    assert_eq!(outside[1].severity, Severity::Warning);
}

#[test]
fn a_loose_cage_is_too_far() {
    let item = jacket(boxed(-1.0, 1.0), boxed(-1.5, 1.5), IDENTITY);
    assert_eq!(
        messages(&item),
        vec![
            (
                Severity::Error,
                "Outer cage is too far from the render mesh. Maximum distance between outer cage and mesh is 0.866000, which is above allowed threshold 0.600000".to_string()
            ),
            (
                Severity::Warning,
                "Outer cage does not closely follow the render mesh. Maximum distance between outer cage and mesh is 0.866, and 0.00% of outer cage vertices are inside the render mesh.".to_string()
            ),
        ]
    );
    assert_eq!(
        cage_distance_failures(&cage_mesh_distances(&item)),
        (0..8).collect::<Vec<_>>()
    );
}

#[test]
fn a_cage_inside_the_mesh_intersects_it() {
    let item = jacket(boxed(-1.0, 1.0), boxed(-0.8, 0.8), IDENTITY);
    let f = run(&item);
    let inside = f
        .iter()
        .find(|f| f.message.starts_with("Outer cage intersects"))
        .unwrap();
    assert_eq!(inside.severity, Severity::Error);
    assert_eq!(inside.measured, Some(Value::Text("100.00%".into())));
    assert_eq!(
        inside.message,
        "Outer cage intersects with the render mesh. 100.00% of outer cage vertices are inside render mesh."
    );
}

#[test]
fn only_the_vertices_moved_off_the_inner_cage_are_measured() {
    // The outer cage is the inner one, reordered, its file centred a
    // stud higher, with corner 7 pulled in: only that corner is measured,
    // the rest would be far too far.
    let inner = boxed(-3.0, 3.0);
    let mut outer = inner.clone();
    outer.positions[7] = [1.1, 1.1, 1.1];
    outer.positions.reverse();
    outer.uvs.reverse();
    for p in &mut outer.positions {
        p[1] += 1.0;
    }
    let mut down = IDENTITY;
    down[1] = -1.0;
    let mut item = jacket(boxed(-1.0, 1.0), outer, down);
    item.cages[0].geometry = inner;
    let d = cage_mesh_distances(&item);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].0, 0);
    assert!((d[0].1 - 0.1 * 3f32.sqrt()).abs() < 1e-4, "{}", d[0].1);
}

#[test]
fn copies_on_a_seam_count_once_toward_the_share_outside() {
    // Corners 0 and 1 copied twice more, all outside a small cage, against
    // corners 2..8 inside it: 2 of 8 corners, not 6 of 12 vertices.
    let mut mesh = boxed(-1.0, 1.0);
    for _ in 0..2 {
        mesh.positions
            .extend([mesh.positions[0], mesh.positions[1]]);
    }
    for p in &mut mesh.positions[2..8] {
        *p = p.map(|c| c * 0.1);
    }
    let item = jacket(mesh, boxed(-0.5, 0.5), IDENTITY);
    assert_eq!(mesh_outside_cage(&item), vec![0, 1, 8, 9, 10, 11]);
    assert!(messages(&item)
        .iter()
        .any(|(_, m)| m.starts_with("25.00% of mesh vertices are outside")));
}

#[test]
fn the_cage_is_measured_where_it_sits_on_the_handle() {
    let mut shifted = boxed(-1.2, 1.2);
    for p in &mut shifted.positions {
        p[0] += 5.0;
    }
    let mut back = IDENTITY;
    back[0] = -5.0;
    assert!(run(&jacket(boxed(-1.0, 1.0), shifted.clone(), back)).is_empty());
    assert!(!run(&jacket(boxed(-1.0, 1.0), shifted, IDENTITY)).is_empty());
}
