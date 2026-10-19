use super::*;

struct Fixture {
    dom: WeakDom,
    model: Model,
    ctx: Ref,
    inner: Ref,
    move_action: Ref,
    nested: Ref,
    b1: Ref,
    button: Ref,
    loose: Ref,
}

/// DataModel > ReplicatedStorage > [Ctx > (Move > B1, B2, B3; Folder > Nested; Inner), Sprint]
/// and DataModel > Workspace > Loose.
fn fixture() -> Fixture {
    let mut dom = WeakDom::new();
    let data_model = dom.new_instance("DataModel", "DataModel", None);
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", Some(data_model));
    let ctx = dom.new_instance(CONTEXT_CLASS, "Ctx", Some(storage));
    let move_action = dom.new_instance(ACTION_CLASS, "Move", Some(ctx));
    dom.set_property(move_action, "Type", Variant::Enum(2))
        .unwrap();

    let b1 = dom.new_instance(BINDING_CLASS, "B1", Some(move_action));
    dom.set_property(b1, "Up", Variant::Enum(119)).unwrap();
    dom.set_property(b1, "Down", Variant::Enum(115)).unwrap();
    dom.set_property(b1, "Left", Variant::Enum(97)).unwrap();
    dom.set_property(b1, "Right", Variant::Enum(100)).unwrap();

    let b2 = dom.new_instance(BINDING_CLASS, "B2", Some(move_action));
    dom.set_property(b2, "KeyCode", Variant::Enum(115)).unwrap();
    dom.set_property(b2, "PrimaryModifier", Variant::Enum(306))
        .unwrap();

    let button = dom.new_instance("TextButton", "Sprint", Some(storage));
    let b3 = dom.new_instance(BINDING_CLASS, "B3", Some(move_action));
    dom.set_property(b3, "UIButton", Variant::Ref(button))
        .unwrap();

    let folder = dom.new_instance("Folder", "Folder", Some(ctx));
    let nested = dom.new_instance(ACTION_CLASS, "Nested", Some(folder));
    let inner = dom.new_instance(CONTEXT_CLASS, "Inner", Some(ctx));

    let workspace = dom.new_instance("Workspace", "Workspace", Some(data_model));
    let loose = dom.new_instance(ACTION_CLASS, "Loose", Some(workspace));

    let parsed = model(&dom, ReflectionDatabase::shared());
    Fixture {
        dom,
        model: parsed,
        ctx,
        inner,
        move_action,
        nested,
        b1,
        button,
        loose,
    }
}

/// The only binding of a loose action whose configuration `configure` sets up.
fn binding(configure: impl FnOnce(&mut WeakDom, Ref)) -> Binding {
    let mut dom = WeakDom::new();
    let action = dom.new_instance(ACTION_CLASS, "Act", None);
    let binding = dom.new_instance(BINDING_CLASS, "Bind", Some(action));
    configure(&mut dom, binding);
    model(&dom, ReflectionDatabase::shared())
        .loose
        .remove(0)
        .bindings
        .remove(0)
}

fn summary_of(configure: impl FnOnce(&mut WeakDom, Ref)) -> String {
    binding(configure).summary()
}

fn set_key(dom: &mut WeakDom, binding: Ref, property: &str, value: u32) {
    dom.set_property(binding, property, Variant::Enum(value))
        .unwrap();
}

#[test]
fn family_label_names_each_column() {
    assert_eq!(Family::KeyboardMouse.label(), "Keyboard & Mouse");
    assert_eq!(Family::Gamepad.label(), "Gamepad");
    assert_eq!(Family::Touch.label(), "Touch");
}

#[test]
fn action_kind_maps_each_dump_enum_value() {
    assert_eq!(ActionKind::from_value(0), ActionKind::Bool);
    assert_eq!(ActionKind::from_value(1), ActionKind::Direction1D);
    assert_eq!(ActionKind::from_value(2), ActionKind::Direction2D);
    assert_eq!(ActionKind::from_value(3), ActionKind::Direction3D);
    assert_eq!(ActionKind::from_value(4), ActionKind::ViewportPosition);
}

#[test]
fn action_kind_keeps_unknown_enum_value_as_other() {
    assert_eq!(ActionKind::from_value(9), ActionKind::Other(9));
}

#[test]
fn viewport_position_expects_keyboard_mouse_and_touch_only() {
    assert_eq!(
        ActionKind::ViewportPosition.expected_families(),
        &[Family::KeyboardMouse, Family::Touch]
    );
}

#[test]
fn bool_action_expects_every_family() {
    assert_eq!(ActionKind::Bool.expected_families(), &Family::ALL);
}

#[test]
fn unknown_action_kind_expects_every_family() {
    assert_eq!(ActionKind::Other(7).expected_families(), &Family::ALL);
}

#[test]
fn family_of_key_places_keyboard_and_mouse_keys() {
    let db = ReflectionDatabase::shared();
    assert_eq!(family_of_key(db, "W"), Some(Family::KeyboardMouse));
    assert_eq!(family_of_key(db, "LeftShift"), Some(Family::KeyboardMouse));
    assert_eq!(
        family_of_key(db, "MouseLeftButton"),
        Some(Family::KeyboardMouse)
    );
}

#[test]
fn family_of_key_places_gamepad_keys() {
    let db = ReflectionDatabase::shared();
    assert_eq!(family_of_key(db, "ButtonA"), Some(Family::Gamepad));
    assert_eq!(family_of_key(db, "DPadUp"), Some(Family::Gamepad));
    assert_eq!(family_of_key(db, "Thumbstick1"), Some(Family::Gamepad));
}

#[test]
fn family_of_key_places_touch_keys() {
    assert_eq!(
        family_of_key(ReflectionDatabase::shared(), "TouchPosition"),
        Some(Family::Touch)
    );
}

#[test]
fn family_of_key_is_none_for_none_and_unknown_names() {
    let db = ReflectionDatabase::shared();
    assert_eq!(family_of_key(db, "None"), None);
    assert_eq!(family_of_key(db, "Nope"), None);
}

#[test]
fn model_lists_every_context_depth_first() {
    let f = fixture();
    let names: Vec<&str> = f.model.contexts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Ctx", "Inner"]);
    assert_eq!(f.model.contexts[0].referent, f.ctx);
    assert_eq!(f.model.contexts[1].referent, f.inner);
}

#[test]
fn model_context_defaults_are_enabled_priority_1000_no_sink() {
    let f = fixture();
    let ctx = &f.model.contexts[0];
    assert!(ctx.enabled);
    assert_eq!(ctx.priority, 1000);
    assert!(!ctx.sink);
}

#[test]
fn model_context_stored_values_override_defaults() {
    let mut f = fixture();
    f.dom
        .set_property(f.ctx, "Enabled", Variant::Bool(false))
        .unwrap();
    f.dom
        .set_property(f.ctx, "Priority", Variant::Int32(5))
        .unwrap();
    f.dom
        .set_property(f.ctx, "Sink", Variant::Bool(true))
        .unwrap();
    let ctx = &model(&f.dom, ReflectionDatabase::shared()).contexts[0];
    assert!(!ctx.enabled);
    assert_eq!(ctx.priority, 5);
    assert!(ctx.sink);
}

#[test]
fn model_collects_actions_under_context_including_nested_folder_actions() {
    let f = fixture();
    let names: Vec<&str> = f.model.contexts[0]
        .actions
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(names, ["Move", "Nested"]);
    assert_eq!(f.model.contexts[0].actions[1].referent, f.nested);
}

#[test]
fn model_action_without_type_is_bool_and_enabled() {
    let f = fixture();
    let loose = &f.model.loose[0];
    assert_eq!(loose.referent, f.loose);
    assert_eq!(loose.kind, ActionKind::Bool);
    assert!(loose.enabled);
}

#[test]
fn model_action_stored_type_sets_kind() {
    let f = fixture();
    assert_eq!(f.model.contexts[0].actions[0].kind, ActionKind::Direction2D);
}

#[test]
fn model_collects_actions_outside_any_context_as_loose() {
    let f = fixture();
    assert_eq!(f.model.loose.len(), 1);
    assert_eq!(f.model.loose[0].name, "Loose");
}

#[test]
fn model_binding_children_keep_child_order() {
    let f = fixture();
    let names: Vec<&str> = f.model.contexts[0].actions[0]
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(names, ["B1", "B2", "B3"]);
}

#[test]
fn model_binding_defaults_are_thresholds_scales_and_no_inputs() {
    let f = fixture();
    let b3 = &f.model.contexts[0].actions[0].bindings[2];
    assert!(b3.keys.is_empty());
    assert!(b3.modifiers.is_empty());
    assert_eq!(b3.pressed_threshold, 0.5);
    assert_eq!(b3.released_threshold, 0.2);
    assert_eq!(b3.response_curve, 1.0);
    assert_eq!(b3.scale, 1.0);
    assert!(!b3.vector2_scaled);
    assert!(!b3.vector3_scaled);
    assert!(!b3.scriptable);
}

#[test]
fn model_binding_stored_threshold_overrides_default() {
    let mut f = fixture();
    f.dom
        .set_property(f.b1, "PressedThreshold", Variant::Float32(0.8))
        .unwrap();
    let b1 = &model(&f.dom, ReflectionDatabase::shared()).contexts[0].actions[0].bindings[0];
    assert_eq!(b1.pressed_threshold, 0.8);
}

#[test]
fn model_binding_composite_keys_list_in_key_property_order() {
    let f = fixture();
    let keys = &f.model.contexts[0].actions[0].bindings[0].keys;
    assert_eq!(
        keys,
        &[
            ("Up", "W".to_owned()),
            ("Down", "S".to_owned()),
            ("Left", "A".to_owned()),
            ("Right", "D".to_owned()),
        ]
    );
}

#[test]
fn model_binding_keyboard_composite_has_keyboard_family_only() {
    let f = fixture();
    assert_eq!(
        f.model.contexts[0].actions[0].bindings[0].families,
        vec![Family::KeyboardMouse]
    );
}

#[test]
fn model_binding_modifiers_are_listed_by_item_name() {
    let f = fixture();
    let b2 = &f.model.contexts[0].actions[0].bindings[1];
    assert_eq!(b2.keys, vec![("KeyCode", "S".to_owned())]);
    assert_eq!(b2.modifiers, vec!["LeftControl".to_owned()]);
    assert_eq!(b2.families, vec![Family::KeyboardMouse]);
}

#[test]
fn model_binding_ui_button_points_at_text_button_and_adds_touch() {
    let f = fixture();
    let b3 = &f.model.contexts[0].actions[0].bindings[2];
    assert_eq!(b3.ui_button, Some((f.button, "Sprint".to_owned())));
    assert_eq!(b3.families, vec![Family::Touch]);
}

#[test]
fn model_binding_gamepad_key_has_gamepad_family() {
    let mut f = fixture();
    let b1 = f.b1;
    f.dom
        .set_property(b1, "KeyCode", Variant::Enum(1002))
        .unwrap();
    let parsed = model(&f.dom, ReflectionDatabase::shared());
    assert!(parsed.contexts[0].actions[0].bindings[0]
        .families
        .contains(&Family::Gamepad));
}

#[test]
fn model_scriptable_binding_has_no_families() {
    let b = binding(|dom, binding| {
        set_key(dom, binding, "KeyCode", 119);
        set_key(dom, binding, "Type", 1);
    });
    assert!(b.scriptable);
    assert!(b.families.is_empty());
}

#[test]
fn summary_of_single_key_is_its_item_name() {
    assert_eq!(summary_of(|dom, b| set_key(dom, b, "KeyCode", 101)), "E");
}

#[test]
fn summary_of_composite_keys_joins_with_slashes() {
    let s = summary_of(|dom, b| {
        set_key(dom, b, "Up", 119);
        set_key(dom, b, "Down", 115);
        set_key(dom, b, "Left", 97);
        set_key(dom, b, "Right", 100);
    });
    assert_eq!(s, "W/S/A/D");
}

#[test]
fn summary_of_modifier_and_key_joins_with_plus() {
    let s = summary_of(|dom, b| {
        set_key(dom, b, "PrimaryModifier", 306);
        set_key(dom, b, "KeyCode", 115);
    });
    assert_eq!(s, "LeftControl+S");
}

#[test]
fn summary_of_ui_button_is_button_name() {
    let s = summary_of(|dom, b| {
        let button = dom.new_instance("TextButton", "Sprint", None);
        dom.set_property(b, "UIButton", Variant::Ref(button))
            .unwrap();
    });
    assert_eq!(s, "Button: Sprint");
}

#[test]
fn summary_of_key_and_ui_button_separates_with_comma() {
    let s = summary_of(|dom, b| {
        set_key(dom, b, "KeyCode", 101);
        let button = dom.new_instance("TextButton", "Sprint", None);
        dom.set_property(b, "UIButton", Variant::Ref(button))
            .unwrap();
    });
    assert_eq!(s, "E, Button: Sprint");
}

#[test]
fn summary_of_binding_without_input_is_unbound() {
    assert_eq!(summary_of(|_, _| {}), "Unbound");
}

#[test]
fn summary_of_scriptable_binding_is_scriptable() {
    assert_eq!(
        summary_of(|dom, b| set_key(dom, b, "Type", 1)),
        "Scriptable"
    );
}

#[test]
fn context_of_context_is_itself() {
    let f = fixture();
    assert_eq!(f.model.context_of(&f.dom, f.ctx), Some(f.ctx));
}

#[test]
fn context_of_action_is_its_context() {
    let f = fixture();
    assert_eq!(f.model.context_of(&f.dom, f.move_action), Some(f.ctx));
}

#[test]
fn context_of_nested_action_is_nearest_context_above() {
    let f = fixture();
    assert_eq!(f.model.context_of(&f.dom, f.nested), Some(f.ctx));
}

#[test]
fn context_of_binding_is_its_context() {
    let f = fixture();
    assert_eq!(f.model.context_of(&f.dom, f.b1), Some(f.ctx));
}

#[test]
fn context_of_unrelated_instance_is_none() {
    let f = fixture();
    assert_eq!(f.model.context_of(&f.dom, f.button), None);
}

#[test]
fn value_of_returns_stored_value() {
    let mut f = fixture();
    f.dom
        .set_property(f.ctx, "Priority", Variant::Int32(5))
        .unwrap();
    let db = ReflectionDatabase::shared();
    assert!(matches!(
        value_of(&f.dom, db, f.ctx, "Priority"),
        Some(Variant::Int32(5))
    ));
}

#[test]
fn value_of_returns_reflection_default_when_unstored() {
    let f = fixture();
    let db = ReflectionDatabase::shared();
    assert!(matches!(
        value_of(&f.dom, db, f.ctx, "Priority"),
        Some(Variant::Int32(1000))
    ));
}
