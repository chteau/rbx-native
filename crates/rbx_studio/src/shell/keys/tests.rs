use super::*;

fn ctrl_shift() -> Modifiers {
    Modifiers {
        control: true,
        shift: true,
        ..Modifiers::none()
    }
}

#[test]
fn delete_and_backspace_both_delete_with_no_modifiers_required() {
    assert_eq!(
        action_for("delete", Modifiers::none()),
        Some(Action::Delete)
    );
    assert_eq!(
        action_for("backspace", Modifiers::none()),
        Some(Action::Delete)
    );
}

#[test]
fn ctrl_shift_p_and_f_insert_part_and_folder() {
    assert_eq!(action_for("p", ctrl_shift()), Some(Action::InsertPart));
    assert_eq!(action_for("f", ctrl_shift()), Some(Action::InsertFolder));
}

#[test]
fn ctrl_i_opens_the_insert_picker_and_ctrl_shift_i_does_not() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::none()
    };
    assert_eq!(action_for("i", ctrl), Some(Action::Insert));
    assert_eq!(action_for("i", ctrl_shift()), None);
    assert_eq!(action_for("i", Modifiers::none()), None);
}

#[test]
fn f2_renames_only_unmodified() {
    assert_eq!(action_for("f2", Modifiers::none()), Some(Action::Rename));
    assert_eq!(action_for("f2", ctrl_shift()), None);
}

#[test]
fn p_and_f_without_both_modifiers_do_nothing() {
    assert_eq!(action_for("p", Modifiers::none()), None);
    assert_eq!(action_for("f", Modifiers::none()), None);
    let shift_only = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    assert_eq!(action_for("p", shift_only), None);
}

#[test]
fn any_other_key_does_nothing() {
    assert_eq!(action_for("w", Modifiers::none()), None);
    assert_eq!(action_for("enter", Modifiers::none()), None);
}

#[test]
fn deleting_the_selection_clears_it() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    let removed = dom.remove(part);
    assert_eq!(selection_after_removal(Some(part), &removed), None);
}

#[test]
fn deleting_a_sibling_keeps_the_selection() {
    let mut dom = WeakDom::new();
    let kept = dom.new_instance("Part", "Kept", None);
    let other = dom.new_instance("Part", "Other", None);
    let removed = dom.remove(other);
    assert_eq!(selection_after_removal(Some(kept), &removed), Some(kept));
}

#[test]
fn deleting_an_ancestor_clears_a_selection_inside_its_subtree() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let child = dom.new_instance("Part", "Part", Some(model));
    let removed = dom.remove(model);
    assert_eq!(selection_after_removal(Some(child), &removed), None);
}

#[test]
fn no_selection_stays_none() {
    assert_eq!(selection_after_removal(None, &[]), None);
}

#[test]
fn inserted_part_gets_studio_s_visible_defaults() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    apply_part_defaults(&mut dom, part, Some(PART_TYPE_BLOCK));

    let instance = dom.get(part).expect("just inserted");
    assert_eq!(
        instance.properties().get("size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 1.2,
            z: 2.0
        }))
    );
    assert_eq!(
        instance.properties().get("Material"),
        Some(&Variant::Enum(256))
    );
    assert_eq!(instance.properties().get("shape"), Some(&Variant::Enum(1)));
    assert!(matches!(
        instance.properties().get("CFrame"),
        Some(Variant::CFrame(_))
    ));
}

/// Mirrors what `Shell::insert_instance`/`insert_part` does for one Part
/// insert-menu item, without needing a real `Shell`/`Context` — the same
/// reason [`apply_part_defaults`]'s own tests above work on a bare
/// `WeakDom` instead.
fn insert_menu_item(class: &str, shape: Option<u32>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let reference = dom.new_instance(class, class, None);
    if let Some(part_shape) = part_defaults_shape(&database(), class, shape) {
        apply_part_defaults(&mut dom, reference, part_shape);
    }
    (dom, reference)
}

/// One test per `shell::ribbon::insert_tiles` Part-menu item. The bug
/// `ROADMAP.md` describes was invisible in the Explorer — every item
/// already showed the right class — and only showed once the viewport
/// resolved the wrong `ShapeKind`, so each of these checks both.
#[test]
fn block_menu_item_is_a_part_that_renders_as_a_box() {
    let (dom, part) = insert_menu_item("Part", None);
    assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
    assert_eq!(
        rbx_viewer::resolved_shape_label(&dom, &database(), part),
        Some("Box")
    );
}

#[test]
fn sphere_menu_item_is_a_part_that_renders_as_a_ball() {
    let (dom, part) = insert_menu_item("Part", Some(PART_TYPE_BALL));
    assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
    assert_eq!(
        rbx_viewer::resolved_shape_label(&dom, &database(), part),
        Some("Ball")
    );
}

#[test]
fn wedge_menu_item_is_a_wedge_part_with_studio_s_visible_defaults() {
    let (dom, part) = insert_menu_item("WedgePart", None);
    let instance = dom.get(part).expect("just inserted");
    assert_eq!(instance.class(), "WedgePart");
    assert_eq!(
        instance.properties().get("Material"),
        Some(&Variant::Enum(256)),
        "Wedge/CornerWedge must get apply_part_defaults too, not just Part"
    );
    assert_eq!(
        rbx_viewer::resolved_shape_label(&dom, &database(), part),
        Some("Wedge")
    );
}

#[test]
fn corner_wedge_menu_item_is_a_corner_wedge_part_with_studio_s_visible_defaults() {
    let (dom, part) = insert_menu_item("CornerWedgePart", None);
    let instance = dom.get(part).expect("just inserted");
    assert_eq!(instance.class(), "CornerWedgePart");
    assert_eq!(
        instance.properties().get("Material"),
        Some(&Variant::Enum(256)),
        "Wedge/CornerWedge must get apply_part_defaults too, not just Part"
    );
    assert_eq!(
        rbx_viewer::resolved_shape_label(&dom, &database(), part),
        Some("CornerWedge")
    );
}

#[test]
fn cylinder_menu_item_is_a_part_that_renders_as_a_cylinder() {
    let (dom, part) = insert_menu_item("Part", Some(PART_TYPE_CYLINDER));
    assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
    assert_eq!(
        rbx_viewer::resolved_shape_label(&dom, &database(), part),
        Some("CylinderX")
    );
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// Counts unmatched `function`/`do` openers against `end` closers by
/// whitespace-splitting the template into tokens — no full Luau parser is
/// needed to catch a template with a stray or missing `end`.
fn opens_and_ends(template: &str) -> (usize, usize) {
    let opens = template
        .split_whitespace()
        .filter(|token| *token == "function" || *token == "do")
        .count();
    let ends = template
        .split_whitespace()
        .filter(|token| *token == "end")
        .count();
    (opens, ends)
}

#[test]
fn a_non_script_class_gets_no_template() {
    let db = database();
    for class in ["Part", "Folder", "Model"] {
        assert_eq!(default_template(&db, class), None);
    }
}

#[test]
fn module_script_gets_the_table_template_others_get_the_plain_script_template() {
    let db = database();
    assert_eq!(default_template(&db, "ModuleScript"), Some(MODULE_TEMPLATE));
    assert_eq!(default_template(&db, "Script"), Some(SCRIPT_TEMPLATE));
    assert_eq!(default_template(&db, "LocalScript"), Some(SCRIPT_TEMPLATE));
}

#[test]
fn inserting_each_script_class_seeds_a_non_empty_starter_source() {
    let db = database();
    for class in ["Script", "LocalScript", "ModuleScript"] {
        let mut dom = WeakDom::new();
        let reference = dom.new_instance(class, class, None);
        let template = default_template(&db, class).expect("a script class has a template");
        assert!(source::write(&mut dom, reference, template));

        let text = source::read(&dom, reference).expect("Source was just written");
        assert!(
            !text.trim().is_empty(),
            "{class}'s starter text must not be empty"
        );
    }
}

#[test]
fn inserting_a_part_gets_no_source_property() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    apply_part_defaults(&mut dom, part, Some(PART_TYPE_BLOCK));

    assert_eq!(default_template(&database(), "Part"), None);
    assert_eq!(
        dom.get(part)
            .expect("just inserted")
            .properties()
            .get(source::SOURCE_PROPERTY),
        None,
        "a Part must never get a Source property"
    );
}

#[test]
fn the_class_module_template_differs_from_the_plain_one() {
    assert_ne!(MODULE_CLASS_TEMPLATE, MODULE_TEMPLATE);
}

#[test]
fn the_class_module_template_is_an_idiomatic_oop_stub() {
    assert!(MODULE_CLASS_TEMPLATE.contains("setmetatable"));
    assert!(MODULE_CLASS_TEMPLATE.contains("__index"));
    assert!(MODULE_CLASS_TEMPLATE.contains(".new("));
}

#[test]
fn every_template_has_balanced_function_do_end_blocks() {
    for template in [SCRIPT_TEMPLATE, MODULE_TEMPLATE, MODULE_CLASS_TEMPLATE] {
        let (opens, ends) = opens_and_ends(template);
        assert_eq!(
            opens, ends,
            "unbalanced function/do/end in template: {template:?}"
        );
    }
}
