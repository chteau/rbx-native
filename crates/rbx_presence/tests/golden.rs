//! Checks the presence schema against the golden files the plugin's Luau
//! encoder is also tested with, so both sides agree on the wire shape.

use rbx_presence::schema::{
    color_for, Client, Edit, Snapshot, UserState, MAX_EDITS, MAX_SELECTION, MAX_TEXT, PALETTE,
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
fn color_for_wraps_the_palette() {
    assert_eq!(color_for(9), PALETTE[1]);
    assert_eq!(color_for(9), "#1c7ed6");
    assert_eq!(color_for(8), PALETTE[0]);
}
