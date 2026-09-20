use super::*;

fn instance() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    (dom, part)
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_reference_with_no_attributes_or_tags_blob_reads_as_empty() {
    let (dom, part) = instance();

    assert!(attributes(&dom, part).is_empty());
    assert!(tags(&dom, part).is_empty());
}

#[test]
fn a_stale_reference_reads_as_empty_rather_than_panicking() {
    let dom = WeakDom::new();
    let gone = Ref::new(1);

    assert!(attributes(&dom, gone).is_empty());
    assert!(tags(&dom, gone).is_empty());
}

#[test]
fn add_attribute_round_trips_every_offered_type() {
    for type_name in ATTRIBUTE_TYPES {
        let (mut dom, part) = instance();
        let value =
            default_value(type_name).unwrap_or_else(|| panic!("{type_name} has no default"));

        add_attribute(&mut dom, part, "Thing", value.clone()).expect("a fresh attribute");

        assert_eq!(
            attributes(&dom, part).get("Thing"),
            Some(&value),
            "{type_name}"
        );
    }
}

#[test]
fn every_attribute_type_has_a_default_and_nothing_else_does() {
    for type_name in ATTRIBUTE_TYPES {
        assert!(default_value(type_name).is_some(), "{type_name}");
    }
    assert_eq!(default_value("NotAType"), None);
}

#[test]
fn adding_a_duplicate_name_is_refused_and_keeps_the_original_value() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Health", Variant::Float64(100.0)).unwrap();

    let result = add_attribute(&mut dom, part, "Health", Variant::Float64(0.0));

    assert!(result.is_err());
    assert_eq!(
        attributes(&dom, part).get("Health"),
        Some(&Variant::Float64(100.0))
    );
}

#[test]
fn attribute_names_are_validated_per_instance_set_attribute() {
    assert!(validate_attribute_name("Health").is_ok());
    assert!(validate_attribute_name("hit.points-2/v_1").is_ok());

    assert!(validate_attribute_name("").is_err(), "empty");
    assert!(validate_attribute_name("Has Space").is_err(), "space");
    assert!(validate_attribute_name("Weird@Symbol").is_err(), "symbol");
    assert!(
        validate_attribute_name("RBXInternal").is_err(),
        "RBX prefix"
    );
    assert!(
        validate_attribute_name(&"A".repeat(101)).is_err(),
        "over 100 chars"
    );
    assert!(
        validate_attribute_name(&"A".repeat(100)).is_ok(),
        "exactly 100 chars"
    );
}

#[test]
fn an_invalid_name_never_reaches_the_dom() {
    let (mut dom, part) = instance();

    assert!(add_attribute(&mut dom, part, "Bad Name", Variant::Bool(true)).is_err());

    assert!(attributes(&dom, part).is_empty());
}

#[test]
fn remove_attribute_drops_exactly_that_one() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "A", Variant::Bool(true)).unwrap();
    add_attribute(&mut dom, part, "B", Variant::Bool(false)).unwrap();

    remove_attribute(&mut dom, part, "A").expect("A exists");

    let remaining = attributes(&dom, part);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining.get("B"), Some(&Variant::Bool(false)));
}

#[test]
fn removing_an_attribute_that_does_not_exist_is_refused() {
    let (mut dom, part) = instance();

    assert!(remove_attribute(&mut dom, part, "Nope").is_err());
}

#[test]
fn rename_attribute_keeps_the_value_under_the_new_name() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Old", Variant::Int32(7)).unwrap();

    rename_attribute(&mut dom, part, "Old", "New").expect("Old exists, New is free");

    let current = attributes(&dom, part);
    assert_eq!(current.get("New"), Some(&Variant::Int32(7)));
    assert!(!current.contains_key("Old"));
}

#[test]
fn renaming_to_the_same_name_is_a_no_op() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Same", Variant::Bool(true)).unwrap();

    rename_attribute(&mut dom, part, "Same", "Same").expect("a no-op rename");

    assert_eq!(
        attributes(&dom, part).get("Same"),
        Some(&Variant::Bool(true))
    );
}

#[test]
fn renaming_onto_an_existing_attribute_is_refused() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "A", Variant::Bool(true)).unwrap();
    add_attribute(&mut dom, part, "B", Variant::Bool(false)).unwrap();

    let result = rename_attribute(&mut dom, part, "A", "B");

    assert!(result.is_err());
    // Neither attribute moved.
    let current = attributes(&dom, part);
    assert_eq!(current.get("A"), Some(&Variant::Bool(true)));
    assert_eq!(current.get("B"), Some(&Variant::Bool(false)));
}

#[test]
fn renaming_a_missing_attribute_is_refused() {
    let (mut dom, part) = instance();

    assert!(rename_attribute(&mut dom, part, "Ghost", "Renamed").is_err());
}

#[test]
fn renaming_to_an_invalid_name_is_refused_and_leaves_the_original() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Old", Variant::Bool(true)).unwrap();

    assert!(rename_attribute(&mut dom, part, "Old", "Bad Name").is_err());

    assert_eq!(
        attributes(&dom, part).get("Old"),
        Some(&Variant::Bool(true))
    );
}

#[test]
fn set_attribute_value_parses_text_shaped_like_the_current_value() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Speed", Variant::Float64(16.0)).unwrap();

    set_attribute_value(&mut dom, &database(), part, "Speed", "32").expect("a plain number");

    assert_eq!(
        attributes(&dom, part).get("Speed"),
        Some(&Variant::Float64(32.0))
    );
}

#[test]
fn set_attribute_value_round_trips_a_composite_type() {
    let (mut dom, part) = instance();
    add_attribute(
        &mut dom,
        part,
        "Offset",
        Variant::Vector3(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
    )
    .unwrap();

    set_attribute_value(&mut dom, &database(), part, "Offset", "1, 2, 3").expect("three numbers");

    assert_eq!(
        attributes(&dom, part).get("Offset"),
        Some(&Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }))
    );
}

#[test]
fn set_attribute_value_on_a_missing_attribute_is_refused() {
    let (mut dom, part) = instance();

    assert!(set_attribute_value(&mut dom, &database(), part, "Ghost", "1").is_err());
}

#[test]
fn set_attribute_value_rejects_text_that_does_not_parse() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Count", Variant::Int32(1)).unwrap();

    assert!(set_attribute_value(&mut dom, &database(), part, "Count", "not a number").is_err());
    // Untouched.
    assert_eq!(
        attributes(&dom, part).get("Count"),
        Some(&Variant::Int32(1))
    );
}

#[test]
fn row_name_round_trips_through_attribute_of_row() {
    assert_eq!(attribute_of_row(&row_name("Health")), Some("Health"));
    assert_eq!(
        attribute_of_row("Position"),
        None,
        "an ordinary property row"
    );
}

#[test]
fn edit_kind_routes_through_the_same_per_type_mapping_ordinary_properties_use() {
    assert_eq!(edit_kind(&Variant::Bool(true)), Some(EditKind::Bool(true)));
    assert!(matches!(
        edit_kind(&Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        })),
        Some(EditKind::Fields { .. })
    ));
}

#[test]
fn a_type_with_no_properties_panel_editor_falls_back_to_read_only() {
    use rbx_dom::{NumberSequence, NumberSequenceKeypoint};

    let sequence = Variant::NumberSequence(NumberSequence {
        keypoints: vec![NumberSequenceKeypoint {
            envelope: 0.0,
            time: 0.0,
            value: 1.0,
        }],
    });

    assert_eq!(edit_kind(&sequence), None);
}

#[test]
fn add_tag_is_idempotent_like_collection_service_add_tag() {
    let (mut dom, part) = instance();

    add_tag(&mut dom, part, "Enemy").expect("a fresh tag");
    add_tag(&mut dom, part, "Enemy").expect("adding it again is a no-op, not an error");

    assert_eq!(tags(&dom, part), vec!["Enemy".to_owned()]);
}

#[test]
fn an_empty_tag_is_refused() {
    let (mut dom, part) = instance();

    assert!(add_tag(&mut dom, part, "").is_err());
    assert!(tags(&dom, part).is_empty());
}

#[test]
fn a_tag_holding_nul_is_refused() {
    let (mut dom, part) = instance();

    assert!(add_tag(&mut dom, part, "bad\0tag").is_err());
}

#[test]
fn remove_tag_drops_exactly_that_one() {
    let (mut dom, part) = instance();
    add_tag(&mut dom, part, "A").unwrap();
    add_tag(&mut dom, part, "B").unwrap();

    remove_tag(&mut dom, part, "A").expect("A is applied");

    assert_eq!(tags(&dom, part), vec!["B".to_owned()]);
}

#[test]
fn removing_a_tag_that_is_not_applied_is_refused() {
    let (mut dom, part) = instance();

    assert!(remove_tag(&mut dom, part, "Nope").is_err());
}

#[test]
fn tags_survive_a_decode_after_several_add_and_remove_operations() {
    let (mut dom, part) = instance();
    add_tag(&mut dom, part, "One").unwrap();
    add_tag(&mut dom, part, "Two").unwrap();
    add_tag(&mut dom, part, "Three").unwrap();
    remove_tag(&mut dom, part, "Two").unwrap();

    assert_eq!(tags(&dom, part), vec!["One".to_owned(), "Three".to_owned()]);
}

/// The Properties panel's filter box searches attribute names alongside the
/// reflected properties, rather than leaving the Attributes section as the
/// one part of the panel a search cannot reach.
#[test]
fn the_filter_box_narrows_attributes_by_name() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "SpawnDelay", Variant::Float64(1.0)).expect("a fresh attribute");
    add_attribute(&mut dom, part, "SpawnCount", Variant::Float64(2.0)).expect("a fresh attribute");
    add_attribute(&mut dom, part, "Colour", Variant::Float64(3.0)).expect("a fresh attribute");

    let names = |filter: &str| -> Vec<String> {
        attributes_matching(&dom, part, filter)
            .into_keys()
            .collect()
    };

    assert_eq!(names("spawn"), ["SpawnCount", "SpawnDelay"]);
    assert_eq!(names("COLOUR"), ["Colour"]);
    assert!(names("nothing").is_empty());
}

/// An empty box is not a filter that matches nothing — it is no filter at
/// all, which is what keeps the section visible until something is typed.
#[test]
fn an_empty_filter_keeps_every_attribute_and_tag() {
    let (mut dom, part) = instance();
    add_attribute(&mut dom, part, "Thing", Variant::Bool(true)).expect("a fresh attribute");
    add_tag(&mut dom, part, "Enemy").expect("a fresh tag");

    for filter in ["", "   "] {
        assert_eq!(attributes_matching(&dom, part, filter).len(), 1);
        assert_eq!(tags_matching(&dom, part, filter), ["Enemy"]);
    }
}

#[test]
fn the_filter_box_narrows_tags_by_name() {
    let (mut dom, part) = instance();
    add_tag(&mut dom, part, "Enemy").expect("a fresh tag");
    add_tag(&mut dom, part, "EnemySpawner").expect("a fresh tag");
    add_tag(&mut dom, part, "Door").expect("a fresh tag");

    assert_eq!(
        tags_matching(&dom, part, "enemy"),
        ["Enemy", "EnemySpawner"]
    );
    assert_eq!(tags_matching(&dom, part, "door"), ["Door"]);
    assert!(tags_matching(&dom, part, "nothing").is_empty());
}
