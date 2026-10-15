//! Checks the presence schema against the golden files the plugin's Luau
//! encoder is also tested with, so both sides agree on the wire shape.

use rbx_presence::schema::{
    assign_colors, color_for, color_slot, Client, Edit, Snapshot, UserState, MAX_EDITS,
    MAX_SELECTION, MAX_TEXT, PALETTE,
};
use serde_json::Value;

fn fixture_path(name: &str) -> String {
    format!(
        "{}/../../plugins/team-presence/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn read_fixture(name: &str) -> Value {
    let text = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|err| panic!("reading {name}: {err}"));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("parsing {name}: {err}"))
}

/// Writes every number as f64, so `1` from the fixture and `1.0` from the
/// serializer compare equal; `serde_json::Value` otherwise keeps integers
/// and floats apart.
fn normalize(value: Value) -> Value {
    match value {
        Value::Number(n) => Value::from(n.as_f64().expect("finite number")),
        Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, normalize(item)))
                .collect(),
        ),
        other => other,
    }
}

fn base_state() -> UserState {
    UserState {
        v: 1,
        user_id: 1,
        name: "Ana".to_owned(),
        color: "#e8590c".to_owned(),
        client: Client::Studio,
        camera: None,
        selection: Vec::new(),
        script: None,
        edits: Vec::new(),
        seen: 0,
    }
}

#[test]
fn studio_state_round_trips_the_golden_file() {
    let original = read_fixture("state_studio.json");
    let state: UserState = serde_json::from_value(original.clone()).expect("decode");
    let written = serde_json::to_value(&state).expect("encode");
    assert_eq!(normalize(written), normalize(original));
}

#[test]
fn native_state_round_trips_and_keeps_optional_keys_omitted() {
    let original = read_fixture("state_native.json");
    let state: UserState = serde_json::from_value(original.clone()).expect("decode");
    assert!(state.camera.is_none());
    assert!(state.script.is_none());
    let written = serde_json::to_value(&state).expect("encode");
    assert!(written.get("camera").is_none());
    assert!(written.get("script").is_none());
    assert_eq!(normalize(written), normalize(original));
}

#[test]
fn snapshot_round_trips_the_golden_file() {
    let original = read_fixture("snapshot.json");
    let snapshot: Snapshot = serde_json::from_value(original.clone()).expect("decode");
    assert_eq!(snapshot.cursor, 7);
    assert_eq!(snapshot.users.len(), 2);
    let written = serde_json::to_value(&snapshot).expect("encode");
    assert_eq!(normalize(written), normalize(original));
}

#[test]
fn null_camera_and_missing_optional_keys_read_with_defaults() {
    let text = r##"{"v":1,"userId":5,"name":"Dee","color":"#1c7ed6","client":"rbx-native","camera":null}"##;
    let state: UserState = serde_json::from_str(text).expect("decode");
    assert!(state.camera.is_none());
    assert!(state.script.is_none());
    assert!(state.selection.is_empty());
    assert!(state.edits.is_empty());
    assert_eq!(state.seen, 0);
}

#[test]
fn clamp_cuts_selection_to_the_limit() {
    let mut state = base_state();
    state.selection = (0..MAX_SELECTION + 50)
        .map(|i| format!("Workspace.P{i}"))
        .collect();
    state.clamp();
    assert_eq!(state.selection.len(), MAX_SELECTION);
    assert_eq!(
        state.selection.last().map(String::as_str),
        Some(format!("Workspace.P{}", MAX_SELECTION - 1).as_str())
    );
}

#[test]
fn clamp_keeps_the_newest_edits() {
    let total = MAX_EDITS + 5;
    let mut state = base_state();
    state.edits = (0..total)
        .map(|i| Edit {
            path: "Workspace.Part".to_owned(),
            property: "Transparency".to_owned(),
            value: "0".to_owned(),
            at: i as u64,
        })
        .collect();
    state.clamp();
    assert_eq!(state.edits.len(), MAX_EDITS);
    assert_eq!(state.edits.first().map(|e| e.at), Some(5));
    assert_eq!(state.edits.last().map(|e| e.at), Some((total - 1) as u64));
}

#[test]
fn clamp_truncates_multibyte_name_on_a_char_boundary() {
    // "€" is three bytes, so 600 bytes is 200 characters; 512 / 3 leaves
    // room for 170 whole characters (510 bytes).
    let mut state = base_state();
    state.name = "€".repeat(200);
    assert_eq!(state.name.len(), 600);
    state.clamp();
    assert!(state.name.len() <= MAX_TEXT);
    assert_eq!(state.name, "€".repeat(170));
}

#[test]
fn palette_matches_the_fixture() {
    let fixture = read_fixture("colors.json");
    let palette: Vec<&str> = fixture["palette"]
        .as_array()
        .expect("palette")
        .iter()
        .map(|c| c.as_str().expect("colour"))
        .collect();
    assert_eq!(palette, PALETTE.to_vec());
}

#[test]
fn color_slots_match_the_fixture_hashes() {
    let fixture = read_fixture("colors.json");
    for case in fixture["hashes"].as_array().expect("hashes") {
        let id = case["userId"].as_u64().expect("userId");
        let slot = case["slot"].as_u64().expect("slot") as usize;
        assert_eq!(color_slot(id), slot, "slot of {id}");
        assert_eq!(color_for(id), PALETTE[slot], "colour of {id}");
    }
}

#[test]
fn assigned_colors_match_the_fixture_rooms() {
    let fixture = read_fixture("colors.json");
    let ids = |value: &Value| -> Vec<u64> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_u64().expect("id"))
            .collect()
    };
    for case in fixture["rooms"].as_array().expect("rooms") {
        let got = assign_colors(&ids(&case["room"]), &ids(&case["extra"]));
        let want = case["colors"].as_object().expect("colors");
        assert_eq!(got.len(), want.len(), "{case}");
        for (key, colour) in want {
            let id: u64 = key.parse().expect("decimal id");
            assert_eq!(
                got.get(&id).copied(),
                colour.as_str(),
                "user {id} in {case}"
            );
        }
    }
}

#[test]
fn a_room_of_eight_gets_eight_distinct_colors() {
    let room: Vec<u64> = (100..108).collect();
    let colors = assign_colors(&room, &[]);
    let distinct: std::collections::HashSet<_> = colors.values().collect();
    assert_eq!(colors.len(), 8);
    assert_eq!(distinct.len(), 8);
}
