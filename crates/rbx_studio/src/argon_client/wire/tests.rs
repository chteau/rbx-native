use rmpv::Value;

use super::*;

fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (Value::from(k), v))
            .collect(),
    )
}

fn bin16(byte: u8) -> Value {
    Value::Binary(vec![byte; 16])
}

#[test]
fn a_message_envelope_is_a_single_key_map_dispatched_by_that_key() {
    let value = map(vec![(
        "SyncDetails",
        map(vec![
            ("name", Value::from("MyPlace")),
            ("version", Value::from("2.0.13")),
        ]),
    )]);
    match Message::decode(&value) {
        Some(Message::SyncDetails(project)) => {
            assert_eq!(project.name, "MyPlace");
            assert_eq!(project.version, "2.0.13");
            assert_eq!(project.game_id, None);
            assert!(project.place_ids.is_empty());
        }
        _ => panic!("expected a SyncDetails message"),
    }
}

#[test]
fn a_published_projects_details_carry_its_game_and_place_ids() {
    let value = map(vec![
        ("name", Value::from("MyPlace")),
        ("version", Value::from("2.0.13")),
        ("gameId", Value::from(1234u64)),
        (
            "placeIds",
            Value::Array(vec![Value::from(56u64), Value::from(78u64)]),
        ),
    ]);
    let project = Project::decode(&value).expect("a project");
    assert_eq!(project.game_id, Some(1234));
    assert_eq!(project.place_ids, vec![56, 78]);
}

#[test]
fn execute_code_decodes_but_carries_nothing_to_run() {
    let value = map(vec![(
        "ExecuteCode",
        map(vec![("code", Value::from("os.execute('rm -rf /')"))]),
    )]);
    assert!(matches!(
        Message::decode(&value),
        Some(Message::ExecuteCode)
    ));
}

#[test]
fn disconnect_carries_its_message_through() {
    let value = map(vec![(
        "Disconnect",
        map(vec![("message", Value::from("bye"))]),
    )]);
    match Message::decode(&value) {
        Some(Message::Disconnect(reason)) => assert_eq!(reason, "bye"),
        _ => panic!("expected a Disconnect message"),
    }
}

#[test]
fn an_argon_ref_round_trips_as_sixteen_raw_bytes() {
    let value = bin16(7);
    let decoded = ArgonRef::decode(&value).expect("a 16-byte bin decodes");
    assert_eq!(decoded, ArgonRef([7; 16]));
    assert_eq!(decoded.encode(), value);
}

#[test]
fn a_ref_of_the_wrong_length_does_not_decode() {
    assert!(ArgonRef::decode(&Value::Binary(vec![1, 2, 3])).is_none());
}

#[test]
fn a_snapshot_decodes_its_fields_and_recurses_into_children() {
    let child = map(vec![
        ("id", bin16(2)),
        ("name", Value::from("Child")),
        ("class", Value::from("Part")),
        ("properties", map(vec![])),
        ("children", Value::Array(vec![])),
    ]);
    let root = map(vec![
        ("id", bin16(1)),
        ("name", Value::from("Root")),
        ("class", Value::from("Folder")),
        ("properties", map(vec![])),
        ("children", Value::Array(vec![child])),
    ]);

    let snapshot = Snapshot::decode(&root).expect("a well-formed snapshot decodes");
    assert_eq!(snapshot.id, ArgonRef([1; 16]));
    assert_eq!(snapshot.name, "Root");
    assert_eq!(snapshot.class, "Folder");
    assert!(
        snapshot.parent.is_none(),
        "a snapshot node carries no parent field"
    );
    assert_eq!(snapshot.children.len(), 1);
    assert_eq!(snapshot.children[0].name, "Child");
}

#[test]
fn an_addition_carries_its_own_parent_field() {
    let value = map(vec![
        ("id", bin16(1)),
        ("parent", bin16(0)),
        ("name", Value::from("Root")),
        ("class", Value::from("Folder")),
        ("properties", map(vec![])),
        ("children", Value::Array(vec![])),
    ]);
    let snapshot = Snapshot::decode(&value).unwrap();
    assert_eq!(snapshot.parent, Some(ArgonRef::ROOT));
}

#[test]
fn the_argon_empty_sentinel_property_is_dropped_not_stored() {
    let properties = map(vec![("ArgonEmpty", map(vec![("Bool", Value::from(true))]))]);
    assert_eq!(decode_properties(&properties), Vec::new());
}

#[test]
fn a_real_property_survives_decode_properties_unchanged() {
    let properties = map(vec![(
        "Transparency",
        map(vec![("Float32", Value::from(0.5))]),
    )]);
    let decoded = decode_properties(&properties);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].0, "Transparency");
}

#[test]
fn an_updated_snapshot_only_requires_an_id() {
    let value = map(vec![("id", bin16(9))]);
    let updated = UpdatedSnapshot::decode(&value).expect("id alone is enough");
    assert_eq!(updated.id, ArgonRef([9; 16]));
    assert!(updated.name.is_none());
    assert!(updated.class.is_none());
    assert!(updated.properties.is_none());
}

#[test]
fn changes_defaults_every_list_to_empty_rather_than_failing() {
    let changes = Changes::decode(&map(vec![])).expect("an empty Changes object still decodes");
    assert!(changes.is_empty());
    assert_eq!(changes.len(), 0);
}

#[test]
fn changes_counts_every_kind_of_entry() {
    let value = map(vec![
        (
            "additions",
            Value::Array(vec![map(vec![
                ("id", bin16(1)),
                ("name", Value::from("A")),
                ("class", Value::from("Part")),
                ("properties", map(vec![])),
                ("children", Value::Array(vec![])),
            ])]),
        ),
        ("updates", Value::Array(vec![map(vec![("id", bin16(2))])])),
        ("removals", Value::Array(vec![bin16(3)])),
    ]);
    let changes = Changes::decode(&value).unwrap();
    assert_eq!(changes.len(), 3);
    assert!(!changes.is_empty());
}

#[test]
fn a_removal_round_trips_as_a_bare_ref_on_the_wire() {
    let changes = Changes {
        additions: Vec::new(),
        updates: Vec::new(),
        removals: vec![ArgonRef([4; 16])],
    };
    let encoded = changes.encode();
    let removals = map_get(&encoded, "removals").unwrap();
    assert_eq!(removals.as_array().unwrap(), &[bin16(4)]);
}

#[test]
fn an_empty_properties_map_encodes_as_the_argon_empty_sentinel() {
    let snapshot = Snapshot {
        id: ArgonRef([1; 16]),
        parent: None,
        name: "Empty".to_owned(),
        class: "Folder".to_owned(),
        properties: Vec::new(),
        children: Vec::new(),
    };
    let encoded = snapshot.encode();
    let properties = map_get(&encoded, "properties").unwrap();
    assert!(map_get(properties, "ArgonEmpty").is_some());
}
