use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{accepts, movable};

/// `Workspace` holding `Model` holding `Part`, plus a sibling `Folder` and a
/// second service — the smallest tree that can express every rule this module
/// has: a service, a nesting, a cycle, and somewhere legal to land.
struct Place {
    dom: WeakDom,
    workspace: Ref,
    lighting: Ref,
    model: Ref,
    part: Ref,
    folder: Ref,
}

fn place() -> Place {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let lighting = dom.new_instance("Lighting", "Lighting", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let part = dom.new_instance("Part", "Part", Some(model));
    let folder = dom.new_instance("Folder", "Folder", Some(workspace));

    Place {
        dom,
        workspace,
        lighting,
        model,
        part,
        folder,
    }
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_part_moves_into_a_sibling_folder() {
    let place = place();
    assert_eq!(
        movable(&place.dom, &database(), &[place.part], place.folder),
        vec![place.part]
    );
}

#[test]
fn an_instance_never_drops_onto_itself() {
    let place = place();
    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.model],
        place.model
    ));
}

#[test]
fn an_instance_never_drops_into_its_own_child() {
    let place = place();
    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.model],
        place.part
    ));
}

#[test]
fn an_instance_never_drops_into_a_deeper_descendant() {
    let mut place = place();
    let inner = place.dom.new_instance("Folder", "Inner", Some(place.part));

    assert!(!accepts(&place.dom, &database(), &[place.workspace], inner));
    assert!(!accepts(&place.dom, &database(), &[place.model], inner));
}

#[test]
fn a_service_is_never_dragged_anywhere() {
    let place = place();
    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.workspace],
        place.lighting
    ));
    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.lighting],
        place.folder
    ));
}

#[test]
fn a_root_instance_that_is_not_a_service_still_moves() {
    // What a `.rbxm` opens as: ordinary instances sitting at the root with no
    // DataModel above them. Nothing about being a root should pin them there.
    let mut dom = WeakDom::new();
    let loose = dom.new_instance("Model", "Loose", None);
    let folder = dom.new_instance("Folder", "Folder", None);

    assert_eq!(movable(&dom, &database(), &[loose], folder), vec![loose]);
}

#[test]
fn dropping_onto_the_parent_it_already_has_moves_nothing() {
    let place = place();
    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.part],
        place.model
    ));
}

#[test]
fn dropping_onto_an_instance_that_is_gone_moves_nothing() {
    let mut place = place();
    place.dom.remove(place.folder);

    assert!(!accepts(
        &place.dom,
        &database(),
        &[place.part],
        place.folder
    ));
}

#[test]
fn a_dragged_instance_that_is_gone_is_skipped_rather_than_refused() {
    let mut place = place();
    let ghost = Ref::new(9999);
    place.dom.remove(place.model);
    let loose = place
        .dom
        .new_instance("Part", "Loose", Some(place.workspace));

    assert_eq!(
        movable(&place.dom, &database(), &[ghost, loose], place.lighting),
        vec![loose]
    );
}

#[test]
fn dragging_a_parent_and_its_child_together_moves_only_the_parent() {
    let place = place();
    assert_eq!(
        movable(
            &place.dom,
            &database(),
            &[place.model, place.part],
            place.folder
        ),
        vec![place.model]
    );
}

#[test]
fn several_unrelated_instances_all_move() {
    let mut place = place();
    let other = place
        .dom
        .new_instance("Part", "Other", Some(place.workspace));

    assert_eq!(
        movable(
            &place.dom,
            &database(),
            &[place.part, other, place.folder],
            place.lighting
        ),
        vec![place.part, other, place.folder]
    );
}

#[test]
fn a_drop_the_rules_allow_leaves_the_tree_reachable_from_a_root() {
    // The point of every rule above: after the move `WeakDom::set_parent` is
    // told to make, walking down from the roots still reaches everything.
    let mut place = place();
    let moving = movable(&place.dom, &database(), &[place.part], place.lighting);
    for reference in &moving {
        place.dom.set_parent(*reference, Some(place.lighting));
    }

    let mut reached = Vec::new();
    let mut stack: Vec<Ref> = place.dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        assert!(!reached.contains(&current), "cycle reached {current:?}");
        reached.push(current);
        stack.extend_from_slice(place.dom.get(current).expect("a live instance").children());
    }

    assert!(reached.contains(&place.part));
    assert_eq!(place.dom.parent(place.part), Some(place.lighting));
    assert_eq!(reached.len(), 5);
}

#[test]
fn an_empty_drag_moves_nothing() {
    let place = place();
    assert!(!accepts(&place.dom, &database(), &[], place.folder));
}
