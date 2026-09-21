use rbx_dom::Change;
use rbx_reflection::ReflectionDatabase;

use super::*;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn ctrl() -> Modifiers {
    Modifiers {
        control: true,
        ..Modifiers::none()
    }
}

#[test]
fn ctrl_c_v_d_map_to_copy_paste_duplicate() {
    assert_eq!(action_for("c", ctrl()), Some(Action::Copy));
    assert_eq!(action_for("v", ctrl()), Some(Action::Paste));
    assert_eq!(action_for("d", ctrl()), Some(Action::Duplicate));
}

#[test]
fn ctrl_x_is_cut() {
    assert_eq!(action_for("x", ctrl()), Some(Action::Cut));
    assert_eq!(action_for("x", Modifiers::none()), None);
    let shifted = Modifiers {
        shift: true,
        ..ctrl()
    };
    assert_eq!(action_for("x", shifted), None);
}

#[test]
fn ctrl_shift_v_is_paste_into_not_plain_paste() {
    let shifted = Modifiers {
        shift: true,
        ..ctrl()
    };
    assert_eq!(action_for("v", shifted), Some(Action::PasteInto));
    assert_eq!(action_for("v", ctrl()), Some(Action::Paste));
}

#[test]
fn any_other_modifier_on_the_chord_is_not_a_clipboard_action() {
    let with = |f: fn(&mut Modifiers)| {
        let mut modifiers = ctrl();
        f(&mut modifiers);
        modifiers
    };
    let alt = with(|m| m.alt = true);
    let platform = with(|m| m.platform = true);
    let shift = with(|m| m.shift = true);
    for key in ["c", "v", "d"] {
        assert_eq!(action_for(key, alt), None, "Ctrl+Alt+{key}");
        assert_eq!(action_for(key, platform), None, "Ctrl+Super+{key}");
    }
    assert_eq!(action_for("c", shift), None, "Ctrl+Shift+C");
    assert_eq!(action_for("d", shift), None, "Ctrl+Shift+D");
    let shift_alt = with(|m| {
        m.shift = true;
        m.alt = true;
    });
    assert_eq!(action_for("v", shift_alt), None, "Ctrl+Shift+Alt+V");
}

#[test]
fn without_control_nothing_matches() {
    assert_eq!(action_for("c", Modifiers::none()), None);
    assert_eq!(action_for("v", Modifiers::none()), None);
    assert_eq!(action_for("d", Modifiers::none()), None);
}

#[test]
fn an_unrelated_key_does_nothing_even_with_control() {
    assert_eq!(action_for("z", ctrl()), None);
}

#[test]
fn a_service_is_not_copyable() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    assert_eq!(copyable(&dom, &database(), &[workspace]), Vec::<Ref>::new());
}

#[test]
fn a_mixed_selection_keeps_only_the_non_service_entries() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    assert_eq!(copyable(&dom, &database(), &[workspace, part]), vec![part]);
}

#[test]
fn nothing_selected_is_nothing_copyable() {
    let dom = WeakDom::new();
    assert_eq!(copyable(&dom, &database(), &[]), Vec::<Ref>::new());
}

/// What the ribbon greys Copy and Duplicate on: the same guard the two
/// handlers return early from, so a tile is never offered for a click
/// that would do nothing. A selection of services only is the case a
/// plain `selected.is_empty()` would get wrong.
#[test]
fn copy_and_duplicate_are_available_for_exactly_what_the_handlers_act_on() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));

    assert!(!has_copyable(&dom, &database(), &[]), "nothing selected");
    assert!(
        !has_copyable(&dom, &database(), &[workspace]),
        "a service alone is not something to copy"
    );
    assert!(has_copyable(&dom, &database(), &[part]));
    assert!(
        has_copyable(&dom, &database(), &[workspace, part]),
        "a mixed selection still has the part in it"
    );
}

#[test]
fn a_reference_no_longer_in_the_dom_cannot_be_copied() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.remove(part);
    assert_eq!(copyable(&dom, &database(), &[part]), Vec::<Ref>::new());
}

#[test]
fn snapshotting_a_model_carries_its_descendants() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let a = dom.new_instance("Part", "A", Some(model));
    dom.new_instance("Part", "B", Some(a));

    let clipped = snapshot(&dom, model).expect("model resolves");
    assert_eq!(clipped.class, "Model");
    assert_eq!(clipped.children.len(), 1);
    assert_eq!(clipped.children[0].name, "A");
    assert_eq!(clipped.children[0].children[0].name, "B");
}

#[test]
fn a_non_archivable_descendant_and_its_own_subtree_are_excluded() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    dom.new_instance("Part", "Kept", Some(model));
    let skipped = dom.new_instance("Part", "Skipped", Some(model));
    dom.set_property(skipped, ARCHIVABLE, Variant::Bool(false))
        .unwrap();
    dom.new_instance("Part", "GrandchildOfSkipped", Some(skipped));

    let clipped = snapshot(&dom, model).unwrap();
    assert_eq!(clipped.children.len(), 1, "only the archivable child");
    assert_eq!(clipped.children[0].name, "Kept");
}

#[test]
fn a_non_archivable_root_is_still_copied() {
    // Real Studio's Copy/Duplicate ignore the copied instance's *own*
    // Archivable — unlike `Instance:Clone()`, which would refuse it —
    // so this only applies to descendants, never to the thing that was
    // actually selected and copied.
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(part, ARCHIVABLE, Variant::Bool(false))
        .unwrap();

    let clipped = snapshot(&dom, part);
    assert!(clipped.is_some());
}

#[test]
fn the_copy_is_always_archivable_even_if_the_original_was_not() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(part, ARCHIVABLE, Variant::Bool(false))
        .unwrap();

    let clipped = snapshot(&dom, part).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    // Archivable by holding no value: the default, which is `true`.
    let copied = dom.get(copy).unwrap().properties();
    assert_eq!(copied.get(ARCHIVABLE), None);
    assert!(archivable(copied));
    // The original is untouched — only the copy was forced.
    assert_eq!(
        dom.get(part).unwrap().properties().get(ARCHIVABLE),
        Some(&Variant::Bool(false))
    );
}

#[test]
fn materializing_a_model_recreates_its_whole_subtree_under_new_referents() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let original_child = dom.new_instance("Part", "Child", Some(model));

    let clipped = snapshot(&dom, model).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    assert_ne!(copy, model, "the copy is a new instance, not the original");
    let copied_child = dom.get(copy).unwrap().children()[0];
    assert_ne!(copied_child, original_child);
    assert_eq!(dom.get(copied_child).unwrap().name(), "Child");
    assert_eq!(dom.get(copied_child).unwrap().class(), "Part");
}

#[test]
fn duplicating_a_script_carries_its_source_along() {
    // `Source` is just another property in `properties()` (see
    // `script_editor::source`) — no script-specific handling exists
    // anywhere in this module, so this is really a check that the
    // generic property copy above doesn't quietly drop it.
    let mut dom = WeakDom::new();
    let script = dom.new_instance("Script", "Script", None);
    dom.set_property(
        script,
        "Source",
        Variant::String("print(\"hi\")".to_owned()),
    )
    .unwrap();

    let clipped = snapshot(&dom, script).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    assert_eq!(
        dom.get(copy).unwrap().properties().get("Source"),
        Some(&Variant::String("print(\"hi\")".to_owned()))
    );
}

#[test]
fn mutating_the_copy_does_not_affect_the_original() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(part, "Transparency", Variant::Float32(0.0))
        .unwrap();

    let clipped = snapshot(&dom, part).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    dom.set_property(copy, "Transparency", Variant::Float32(1.0))
        .unwrap();

    assert_eq!(
        dom.get(part).unwrap().properties().get("Transparency"),
        Some(&Variant::Float32(0.0)),
        "writing the copy must not reach the original"
    );
    assert_eq!(
        dom.get(copy).unwrap().properties().get("Transparency"),
        Some(&Variant::Float32(1.0))
    );
}

#[test]
fn mutating_the_original_after_copying_does_not_affect_the_copy() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(part, "Transparency", Variant::Float32(0.0))
        .unwrap();

    let clipped = snapshot(&dom, part).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    dom.set_property(part, "Transparency", Variant::Float32(0.5))
        .unwrap();

    assert_eq!(
        dom.get(copy).unwrap().properties().get("Transparency"),
        Some(&Variant::Float32(0.0)),
        "a later edit to the original must not reach the earlier copy"
    );
}

#[test]
fn an_internal_reference_is_remapped_to_the_copy() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let target = dom.new_instance("Part", "Target", Some(model));
    let value = dom.new_instance("ObjectValue", "Link", Some(model));
    dom.set_property(value, "Value", Variant::Ref(target))
        .unwrap();

    let clipped = snapshot(&dom, model).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    let copy_instance = dom.get(copy).unwrap();
    let copied_target = copy_instance
        .children()
        .iter()
        .copied()
        .find(|&r| dom.get(r).unwrap().name() == "Target")
        .unwrap();
    let copied_value = copy_instance
        .children()
        .iter()
        .copied()
        .find(|&r| dom.get(r).unwrap().name() == "Link")
        .unwrap();

    assert_eq!(
        dom.get(copied_value).unwrap().properties().get("Value"),
        Some(&Variant::Ref(copied_target)),
        "a reference to a sibling that was copied along with it must follow the copy"
    );
}

#[test]
fn a_reference_outside_the_copied_subtree_keeps_pointing_at_the_original() {
    let mut dom = WeakDom::new();
    let outside = dom.new_instance("Part", "Outside", None);
    let value = dom.new_instance("ObjectValue", "Link", None);
    dom.set_property(value, "Value", Variant::Ref(outside))
        .unwrap();

    let clipped = snapshot(&dom, value).unwrap();
    let copy = materialize(&mut dom, &clipped, None);

    assert_eq!(
        dom.get(copy).unwrap().properties().get("Value"),
        Some(&Variant::Ref(outside)),
        "a reference to something outside the copy must not be rewritten"
    );
}

#[test]
fn duplicating_two_instances_is_one_batch_of_changes() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(workspace));
    let b = dom.new_instance("Part", "B", Some(workspace));
    dom.take_changes(); // the three creates above, not what's under test

    let clipped_a = snapshot(&dom, a).unwrap();
    let clipped_b = snapshot(&dom, b).unwrap();
    materialize(&mut dom, &clipped_a, Some(workspace));
    materialize(&mut dom, &clipped_b, Some(workspace));
    // One drain, mirroring `Shell::duplicate_selected`'s single
    // `push_history`/`take_changes` pair — this is what makes
    // duplicating a multi-instance selection one undo step rather than
    // one per instance.
    let changes = dom.take_changes();

    let added = changes
        .iter()
        .filter(|change| matches!(change, Change::Added(_)))
        .count();
    assert_eq!(added, 2, "both duplicates land in the same change log");
}
