use glam::Mat3;
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

// The box-edge math itself (`edges`, `EDGES`) now lives in `super::outline`
// and is tested there — this file tests only what selection.rs still owns:
// container resolution, aggregation and the O(N) redraw bookkeeping.

#[test]
fn a_referent_with_no_placement_draws_nothing() {
    // A part the scene never built: one outside `Workspace`, or a `MeshPart`
    // whose real mesh replaced its box (see `Scene::placements`).
    let placements = HashMap::new();
    let vertices = outline::box_edges(&placements, &[part(1)]);
    assert!(vertices.is_empty());
}

#[test]
fn a_part_referent_draws_its_box() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(1), placement(Mat4::IDENTITY));

    let vertices = outline::box_edges(&placements, &[part(1)]);
    assert_eq!(vertices.len(), 72);
}

#[test]
fn an_empty_selection_draws_nothing() {
    let placements = HashMap::new();
    let vertices = outline::box_edges(&placements, &[]);
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
    let vertices = outline::box_edges(&placements, std::slice::from_ref(&selected));
    assert_eq!(vertices.len(), 72);

    // Spanning -3 to 5 in x and -1 to 1 in y and z: the union of two
    // two-stud cubes three studs and five studs from the origin.
    let model = outline::box_of(&placements, &selected).unwrap();
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
    assert!(outline::box_edges(&placements, std::slice::from_ref(&selected)).is_empty());
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

    let outline = outline::box_of(&placements, &selected)
        .unwrap()
        .w_axis
        .truncate();
    let centre = gizmo::centre_of(outline::models_of(&placements, &selected)).unwrap();
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

    let anchor = anchor_of(&placements, &[part(1)]).unwrap();
    assert_eq!(anchor.w_axis.truncate(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(Mat3::from_mat4(anchor), Mat3::from_mat4(model));
}

/// The whole point of an anchor at all: several parts selected together
/// still get exactly one gizmo, at the first one in selection order.
#[test]
fn several_parts_anchor_at_the_first_one_in_selection_order() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(1), placement(Mat4::from_translation(Vec3::X)));
    placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::Y)));
    placements.insert(Ref::new(3), placement(Mat4::from_translation(Vec3::Z)));

    let anchor = anchor_of(&placements, &[part(2), part(1), part(3)]).unwrap();
    assert_eq!(anchor.w_axis.truncate(), Vec3::Y);
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

    let anchor = anchor_of(&placements, &[selected]).unwrap();
    assert_eq!(anchor.w_axis.truncate(), Vec3::X);
}

/// A referent with nothing drawn under it must not hide the gizmo — the
/// search skips it for the next one that does have something.
#[test]
fn a_referent_with_no_placement_is_skipped_rather_than_hiding_the_gizmo() {
    let mut placements = HashMap::new();
    placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::X)));

    let anchor = anchor_of(&placements, &[part(1), part(2)]).unwrap();
    assert_eq!(anchor.w_axis.truncate(), Vec3::X);
}

#[test]
fn a_selection_with_no_placement_at_all_anchors_nothing() {
    let placements = HashMap::new();
    assert_eq!(anchor_of(&placements, &[part(1), part(2)]), None);
}

/// A `Model` holding `parts` `Part`s, in a DOM the caller can go on adding to.
fn model_in(dom: &mut WeakDom, parts: usize) -> Ref {
    let model = dom.new_instance("Model", "Model", None);
    for index in 0..parts {
        dom.new_instance("Part", &format!("Part{index}"), Some(model));
    }
    model
}

/// The box a `BasePart` gets is its own, hugging it however it is turned —
/// not the loose world-axis-aligned box a container gets, even where parts
/// are parented under it (a welded assembly, a `Tool`'s `Handle`).
#[test]
fn a_part_with_parts_under_it_keeps_its_own_oriented_box() {
    let mut dom = WeakDom::new();
    let handle = dom.new_instance("Part", "Handle", None);
    let sight = dom.new_instance("Part", "Sight", Some(handle));
    let selected = Selected::read(&dom, &ReflectionDatabase::embedded(), handle);

    // Turned an eighth of a turn: a box around the two of them would be both
    // wider than the part and square to the world rather than to the part.
    let turned = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let mut placements = HashMap::new();
    placements.insert(handle, placement(turned));
    placements.insert(sight, cube(Vec3::new(9.0, 0.0, 0.0)));

    assert_eq!(outline::box_of(&placements, &selected), Some(turned));
    assert_eq!(outline::box_edges(&placements, &[selected]).len(), 72);
}

/// A model selected together with one of its own parts is one box, not two
/// drawn over each other: `pick::selection` drops the covered entry, and the
/// outline is built from what it kept.
#[test]
fn a_model_and_a_part_inside_it_draw_one_box() {
    let mut dom = WeakDom::new();
    let model = model_in(&mut dom, 2);
    let database = ReflectionDatabase::embedded();
    let inside: Vec<Ref> = crate::pick::parts_of(&dom, &database, model).collect();

    let selected = crate::pick::selection(&dom, &database, &[model, inside[0]]);
    let mut placements = HashMap::new();
    for (index, &referent) in inside.iter().enumerate() {
        placements.insert(referent, cube(Vec3::new(index as f32 * 6.0, 0.0, 0.0)));
    }

    assert_eq!(outline::box_edges(&placements, &selected).len(), 72);
}

/// What a drag of a whole model costs: each of its parts arrives through
/// `place` in turn, and rebuilding the aggregate box for every one of them
/// would make a single drag step quadratic in the number of parts. One
/// rebuild per frame, however many moved.
#[test]
fn a_group_drag_rebuilds_the_outline_once_however_many_parts_moved() {
    let selected = model_holding(64);
    let mut outline = Outline::default();
    outline.set(std::slice::from_ref(&selected));
    assert!(
        outline.take_vertices().is_some(),
        "a fresh selection owes its first box"
    );

    for (index, &referent) in selected.parts().iter().enumerate() {
        outline.place(referent, cube(Vec3::new(index as f32, 0.0, 0.0)));
    }

    assert!(outline.take_vertices().is_some());
    assert!(
        outline.take_vertices().is_none(),
        "and nothing more until something moves again"
    );
}

/// A part the selection does not cover moves constantly — every other
/// instance a script or a drag touches — and must not cost the outline a
/// rebuild.
#[test]
fn a_part_outside_the_selection_owes_the_outline_nothing() {
    let selected = model_holding(2);
    let mut outline = Outline::default();
    outline.set(std::slice::from_ref(&selected));
    outline.take_vertices();

    outline.place(Ref::new(999), cube(Vec3::ZERO));
    assert!(outline.take_vertices().is_none());
}

/// The box still follows the parts that moved under it, which is the whole
/// reason `place` marks it stale at all.
#[test]
fn the_box_follows_a_part_that_moved_under_it() {
    let selected = model_holding(2);
    let [first, second] = [selected.parts()[0], selected.parts()[1]];
    let mut outline = Outline::default();
    outline.set(std::slice::from_ref(&selected));
    outline.place(first, cube(Vec3::ZERO));
    outline.place(second, cube(Vec3::new(4.0, 0.0, 0.0)));
    outline.take_vertices();

    outline.place(second, cube(Vec3::new(20.0, 0.0, 0.0)));
    assert!(outline.take_vertices().is_some());
    let widened = outline::box_of(&outline.placements, &selected).expect("both parts placed");
    assert!((widened.x_axis.length() - 22.0).abs() < 1e-4);
}

/// A reload that parents another `Part` under the selected model widens the
/// box and moves the gizmo with it — the renderer holds no DOM to notice on
/// its own, so it is told by being sent the selection again (see
/// `rbxstudio`'s `Shell::sync_viewport_selection`).
#[test]
fn a_model_that_gained_a_part_is_outlined_around_it_once_resent() {
    let mut dom = WeakDom::new();
    let model = model_in(&mut dom, 2);
    let database = ReflectionDatabase::embedded();
    let before = Selected::read(&dom, &database, model);

    let mut outline = Outline::default();
    for (index, &referent) in before.parts().iter().enumerate() {
        outline.place(referent, cube(Vec3::new(index as f32 * 4.0, 0.0, 0.0)));
    }
    outline.set(std::slice::from_ref(&before));
    outline.take_vertices();
    let narrow = outline::box_of(&outline.placements, &before).expect("both parts placed");
    assert!((narrow.x_axis.length() - 6.0).abs() < 1e-4);

    // What the script did, and what the reload then has to re-resolve.
    let added = dom.new_instance("Part", "Part2", Some(model));
    let after = Selected::read(&dom, &database, model);
    assert_eq!(after.parts().len(), 3);
    outline.place(added, cube(Vec3::new(20.0, 0.0, 0.0)));
    outline.set(std::slice::from_ref(&after));

    assert!(outline.take_vertices().is_some());
    let wide = outline::box_of(&outline.placements, &after).expect("all three parts placed");
    assert!((wide.x_axis.length() - 22.0).abs() < 1e-4, "{wide}");
    assert!((wide.w_axis.truncate().x - 10.0).abs() < 1e-4, "{wide}");
}

/// A `Model` inside a `Model` is not a second box: the outer one's aggregate
/// spans every part at every depth, because `pick::parts_of` descends the
/// whole subtree rather than stopping at the first container it meets. The
/// nested model itself contributes nothing of its own — it is a `PVInstance`
/// but not a `BasePart`, so it has no `Size` to union in, and its parts are
/// already counted.
#[test]
fn a_model_nested_in_a_model_is_spanned_by_the_outer_ones_box() {
    let mut dom = WeakDom::new();
    let outer = dom.new_instance("Model", "House", None);
    let near = dom.new_instance("Part", "Wall", Some(outer));
    let inner = dom.new_instance("Model", "Door", Some(outer));
    let deep = dom.new_instance("Part", "Handle", Some(inner));
    let database = ReflectionDatabase::embedded();

    let mut placements = HashMap::new();
    placements.insert(near, cube(Vec3::new(-3.0, 0.0, 0.0)));
    placements.insert(deep, cube(Vec3::new(9.0, 0.0, 0.0)));

    let selected = Selected::read(&dom, &database, outer);
    assert_eq!(selected.parts().len(), 2, "the buried part counts too");

    // Two two-stud cubes at -3 and 9: one box spanning -4..10.
    let model = outline::box_of(&placements, &selected).expect("both parts placed");
    assert!((model.x_axis.length() - 14.0).abs() < 1e-4, "{model}");
    assert!((model.w_axis.truncate() - Vec3::new(3.0, 0.0, 0.0)).length() < 1e-4);
    assert_eq!(outline::box_edges(&placements, &[selected]).len(), 72);

    // And selecting the nested model alone is its own, smaller box — the
    // outer one's extent is never inherited downwards.
    let inside = Selected::read(&dom, &database, inner);
    let model = outline::box_of(&placements, &inside).expect("the buried part is placed");
    assert!((model.x_axis.length() - 2.0).abs() < 1e-4, "{model}");
}
