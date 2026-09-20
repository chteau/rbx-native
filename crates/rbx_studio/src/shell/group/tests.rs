use rbx_dom::Change;
use rbx_reflection::ReflectionDatabase;

use super::*;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn grouping_wraps_the_selection_under_one_new_model_and_nothing_else_moves() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(workspace));
    let b = dom.new_instance("Part", "B", Some(workspace));
    let untouched = dom.new_instance("Part", "Untouched", Some(workspace));

    let parent = common_parent(&dom, &database(), &[a, b]).expect("shares one parent");
    let model = apply_group(&mut dom, &[a, b], parent);

    assert_eq!(dom.parent(a), Some(model));
    assert_eq!(dom.parent(b), Some(model));
    assert_eq!(dom.parent(model), Some(workspace));
    assert_eq!(
        dom.parent(untouched),
        Some(workspace),
        "an unselected sibling must not move"
    );
}

#[test]
fn grouping_is_one_batch_of_changes_for_the_whole_selection() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(workspace));
    let b = dom.new_instance("Part", "B", Some(workspace));
    dom.take_changes(); // the three creates above, not what's under test

    let model = apply_group(&mut dom, &[a, b], Some(workspace));
    // One drain, mirroring `Shell::group_selected`'s single
    // `push_history`/`take_changes` pair — this is what makes a Group
    // one undo step rather than one per instance.
    let changes = dom.take_changes();

    assert_eq!(
        changes,
        vec![
            Change::Added(model),
            Change::Parent {
                referent: model,
                old: None,
                new: Some(workspace),
            },
            Change::Parent {
                referent: a,
                old: Some(workspace),
                new: Some(model),
            },
            Change::Parent {
                referent: b,
                old: Some(workspace),
                new: Some(model),
            },
        ]
    );
}

#[test]
fn grouping_a_selection_spanning_multiple_parents_is_refused() {
    let mut dom = WeakDom::new();
    let a_parent = dom.new_instance("Model", "A", None);
    let b_parent = dom.new_instance("Model", "B", None);
    let a = dom.new_instance("Part", "PartA", Some(a_parent));
    let b = dom.new_instance("Part", "PartB", Some(b_parent));

    assert_eq!(common_parent(&dom, &database(), &[a, b]), None);
}

#[test]
fn grouping_a_service_is_refused() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));

    assert_eq!(common_parent(&dom, &database(), &[workspace, part]), None);
}

#[test]
fn grouping_nothing_selected_is_refused() {
    let dom = WeakDom::new();
    assert_eq!(common_parent(&dom, &database(), &[]), None);
}

#[test]
fn ungrouping_reparents_children_onto_the_model_s_old_parent_and_removes_it() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let a = dom.new_instance("Part", "A", Some(model));
    let b = dom.new_instance("Part", "B", Some(model));

    let (parent, children) =
        ungroupable(&dom, &database(), model).expect("a model with children ungroups");
    assert_eq!(parent, Some(workspace));
    assert_eq!(children, vec![a, b]);

    let freed = apply_ungroup(&mut dom, &[(model, parent, children)]);

    assert_eq!(freed, vec![a, b]);
    assert_eq!(dom.parent(a), Some(workspace));
    assert_eq!(dom.parent(b), Some(workspace));
    assert!(dom.get(model).is_none(), "the emptied Model is removed");
}

#[test]
fn ungrouping_is_one_batch_of_changes() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let a = dom.new_instance("Part", "A", Some(model));
    let b = dom.new_instance("Part", "B", Some(model));
    dom.take_changes(); // the four creates above, not what's under test

    apply_ungroup(&mut dom, &[(model, Some(workspace), vec![a, b])]);
    // One drain, mirroring `Shell::ungroup_selected`'s single
    // `push_history`/`take_changes` pair.
    let changes = dom.take_changes();

    assert_eq!(
        changes,
        vec![
            Change::Parent {
                referent: a,
                old: Some(model),
                new: Some(workspace),
            },
            Change::Parent {
                referent: b,
                old: Some(model),
                new: Some(workspace),
            },
            Change::Removed(model),
        ]
    );
}

#[test]
fn ungrouping_a_non_model_is_refused() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    assert_eq!(ungroupable(&dom, &database(), part), None);
}

#[test]
fn ungrouping_an_empty_model_is_refused() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Empty", None);
    assert_eq!(ungroupable(&dom, &database(), model), None);
}

#[test]
fn ungrouping_the_workspace_itself_is_refused() {
    // `Workspace` is a `Model` subclass in Roblox's own class hierarchy
    // (see `shell::selection`'s own note on the same fact); the service
    // check in `ungroupable` is what stops this from being treated as
    // an ordinary, ungroupable `Model`.
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Part", "Part", Some(workspace));
    assert_eq!(ungroupable(&dom, &database(), workspace), None);
}

#[test]
fn ctrl_g_groups_and_ctrl_shift_g_ungroups() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::none()
    };
    let ctrl_shift = Modifiers {
        control: true,
        shift: true,
        ..Modifiers::none()
    };
    assert_eq!(action_for("g", ctrl), Some(Action::Group));
    assert_eq!(action_for("g", ctrl_shift), Some(Action::Ungroup));
    assert_eq!(action_for("g", Modifiers::none()), None);
}

/// What the ribbon greys Group on: `common_parent`'s own answer, so the
/// tile is live exactly when `group_selected` would wrap something.
#[test]
fn group_is_available_for_exactly_what_the_handler_wraps() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(workspace));
    let b = dom.new_instance("Part", "B", Some(workspace));
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let nested = dom.new_instance("Part", "Nested", Some(model));

    assert!(!has_groupable(&dom, &database(), &[]), "nothing selected");
    assert!(has_groupable(&dom, &database(), &[a]));
    assert!(has_groupable(&dom, &database(), &[a, b]), "one parent");
    assert!(
        !has_groupable(&dom, &database(), &[a, nested]),
        "two parents cannot be wrapped in one Model"
    );
    assert!(
        !has_groupable(&dom, &database(), &[workspace]),
        "a service cannot be reparented"
    );
}

/// And Ungroup: one `Model` with something in it is enough, because the
/// handler unwraps those and ignores the rest of the selection.
#[test]
fn ungroup_is_available_when_one_selected_model_would_unwrap() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    let empty = dom.new_instance("Model", "Empty", Some(workspace));
    let full = dom.new_instance("Model", "Full", Some(workspace));
    dom.new_instance("Part", "Inside", Some(full));

    assert!(!has_ungroupable(&dom, &database(), &[]));
    assert!(!has_ungroupable(&dom, &database(), &[part]), "not a Model");
    assert!(
        !has_ungroupable(&dom, &database(), &[empty]),
        "an empty Model has nothing to unwrap"
    );
    assert!(has_ungroupable(&dom, &database(), &[full]));
    assert!(
        has_ungroupable(&dom, &database(), &[part, full]),
        "one Model among the rest is enough"
    );
    assert!(
        !has_ungroupable(&dom, &database(), &[workspace]),
        "Workspace is a Model subclass but a service"
    );
}
