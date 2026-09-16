use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::scene::ShapeKind;

fn placement(model: Mat4) -> Placement {
    Placement {
        kind: ShapeKind::Box,
        model,
        size: Vec3::ONE,
    }
}

/// A `BasePart` selected in its own right, which is what every entry was
/// before a container could be one.
fn part(referent: u32) -> Selected {
    Selected::part(Ref::new(referent))
}

/// A two-stud cube standing at `centre`.
fn cube(centre: Vec3) -> Placement {
    placement(Mat4::from_translation(centre) * Mat4::from_scale(Vec3::splat(2.0)))
}

/// A `Model` holding `parts` `Part`s (and one `Folder`, which is not drawable
/// and must not reach the outline), resolved through the very
/// `pick::parts_of` the editor resolves a selection with — so these cover the
/// real descent rather than a hand-written list of descendants.
fn model_holding(parts: usize) -> Selected {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    dom.new_instance("Folder", "Folder", Some(model));
    for index in 0..parts {
        dom.new_instance("Part", &format!("Part{index}"), Some(model));
    }
    Selected::read(&dom, &ReflectionDatabase::embedded(), model)
}

#[test]
fn a_box_has_twelve_edges_and_twenty_four_vertices() {
    let vertices = edges(Mat4::IDENTITY);
    assert_eq!(vertices.len(), 24);
    // 12 edges, each contributing exactly one pair of endpoints.
    assert_eq!(EDGES.len(), 12);
}

#[test]
fn the_corners_follow_the_model_matrix() {
    let model = Mat4::from_translation(Vec3::new(66.0, 6.5, -81.0))
        * Mat4::from_scale(Vec3::new(10.0, 13.0, 2.0));
    let vertices = edges(model);

    // Every vertex is a cube corner carried through `model`: half the part's
    // size away from its centre on every axis.
    for vertex in vertices {
        let local = Vec3::from(vertex.position) - Vec3::new(66.0, 6.5, -81.0);
        assert!((local.x.abs() - 5.0).abs() < 1e-4);
        assert!((local.y.abs() - 6.5).abs() < 1e-4);
        assert!((local.z.abs() - 1.0).abs() < 1e-4);
    }
}

#[test]
fn a_referent_with_no_placement_draws_nothing() {
    // A part the scene never built: one outside `Workspace`, or a `MeshPart`
    // whose real mesh replaced its box (see `Scene::placements`).
    let placements = HashMap::new();
    let vertices = vertices_for(&placements, &[part(1)]);
    assert!(vertices.is_empty());
}

#[test]
fn a_part_referent_draws_its_box() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(1), placement(Mat4::IDENTITY));

    let vertices = vertices_for(&placements, &[part(1)]);
    assert_eq!(vertices.len(), 24);
}

#[test]
fn an_empty_selection_draws_nothing() {
    let placements = HashMap::new();
    let vertices = vertices_for(&placements, &[]);
    assert!(vertices.is_empty());
}

/// The bug this resolution exists for: a `Model` is what a viewport click
/// selects, and outlining nothing for one left the user with a highlighted
/// row in the Explorer and an empty viewport.
#[test]
fn a_model_is_outlined_by_one_box_around_every_part_beneath_it() {
    let selected = model_holding(2);
    let [first, second] = [selected.parts()[0], selected.parts()[1]];
    let mut placements = HashMap::new();
    placements.insert(first, cube(Vec3::new(-2.0, 0.0, 0.0)));
    placements.insert(second, cube(Vec3::new(4.0, 0.0, 0.0)));

    // One box for the whole model, not one per part.
    let vertices = vertices_for(&placements, std::slice::from_ref(&selected));
    assert_eq!(vertices.len(), 24);

    // Spanning -3 to 5 in x and -1 to 1 in y and z: the union of two
    // two-stud cubes three studs and five studs from the origin.
    let model = box_of(&placements, &selected).unwrap();
    assert!((model.w_axis.truncate() - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-4);
    assert!((model.x_axis.length() - 8.0).abs() < 1e-4);
    assert!((model.y_axis.length() - 2.0).abs() < 1e-4);
    assert!((model.z_axis.length() - 2.0).abs() < 1e-4);
}

/// A container holding no geometry — an empty `Model`, a `Folder` of scripts,
/// a service — has nothing to draw a box around, and still draws none.
#[test]
fn a_model_with_no_parts_beneath_it_draws_nothing() {
    let selected = model_holding(0);
    assert!(selected.parts().is_empty());

    let placements = HashMap::new();
    assert!(vertices_for(&placements, std::slice::from_ref(&selected)).is_empty());
    assert_eq!(anchor_of(&placements, &[selected]), None);
}

/// Where the outline is drawn and where the Move gizmo stands have to be the
/// same answer, or the handles float off the box they belong to.
#[test]
fn a_models_outline_is_centred_where_its_gizmo_stands() {
    let selected = model_holding(3);
    let mut placements = HashMap::new();
    for (index, &referent) in selected.parts().iter().enumerate() {
        placements.insert(referent, cube(Vec3::splat(index as f32 * 7.0)));
    }

    let outline = box_of(&placements, &selected).unwrap().w_axis.truncate();
    let centre = gizmo::centre_of(models_of(&placements, &selected)).unwrap();
    assert!((outline - centre).length() < 1e-4);
}

#[test]
fn nothing_selected_anchors_nothing() {
    let placements = HashMap::new();
    assert_eq!(anchor_of(&placements, &[]), None);
}

#[test]
fn a_single_parts_anchor_is_its_own_centre_and_rotation() {
    let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
    let mut placements = HashMap::new();
    placements.insert(Ref::new(1), placement(model));

    let (origin, rotation) = anchor_of(&placements, &[part(1)]).unwrap();
    assert_eq!(origin, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(rotation, Mat3::from_mat4(model));
}

/// The whole point of an anchor at all: several parts selected together
/// still get exactly one gizmo, at the first one in selection order.
#[test]
fn several_parts_anchor_at_the_first_one_in_selection_order() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(1), placement(Mat4::from_translation(Vec3::X)));
    placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::Y)));
    placements.insert(Ref::new(3), placement(Mat4::from_translation(Vec3::Z)));

    let (origin, _) = anchor_of(&placements, &[part(2), part(1), part(3)]).unwrap();
    assert_eq!(origin, Vec3::Y);
}

/// Scale and Rotate transform the anchor, and a `Model` has no `Size` or
/// `CFrame` of its own to write one into — so a selected model anchors on the
/// first real part beneath it, exactly as `transform::Targets::read` does on
/// the editor's side.
#[test]
fn a_model_anchors_on_the_first_part_beneath_it() {
    let selected = model_holding(2);
    let mut placements = HashMap::new();
    placements.insert(
        selected.parts()[0],
        placement(Mat4::from_translation(Vec3::X)),
    );
    placements.insert(
        selected.parts()[1],
        placement(Mat4::from_translation(Vec3::Y)),
    );

    let (origin, _) = anchor_of(&placements, &[selected]).unwrap();
    assert_eq!(origin, Vec3::X);
}

/// A referent with nothing drawn under it must not hide the gizmo — the
/// search skips it for the next one that does have something.
#[test]
fn a_referent_with_no_placement_is_skipped_rather_than_hiding_the_gizmo() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::X)));

    let (origin, _) = anchor_of(&placements, &[part(1), part(2)]).unwrap();
    assert_eq!(origin, Vec3::X);
}

#[test]
fn a_selection_with_no_placement_at_all_anchors_nothing() {
    let placements = HashMap::new();
    assert_eq!(anchor_of(&placements, &[part(1), part(2)]), None);
}
