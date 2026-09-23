use super::*;

fn keys() -> LevelKeys {
    LevelKeys {
        game: Some("game-a".to_owned()),
        place: Some("place-1".to_owned()),
    }
}

#[test]
fn every_setting_defaults_to_the_plugins_own_value() {
    let settings = ArgonSettings::default();
    assert_eq!(
        settings.get(Setting::AutoConnect, &keys()),
        Value::Bool(true)
    );
    assert_eq!(
        settings.get(Setting::TwoWaySync, &keys()),
        Value::Bool(false)
    );
    assert_eq!(
        settings.get(Setting::LogLevel, &keys()),
        Value::Choice("Warn")
    );
    assert_eq!(
        settings.get(Setting::ChangesThreshold, &keys()),
        Value::Number(5)
    );
    assert_eq!(
        settings.get(Setting::DiffLinesLimit, &keys()),
        Value::Number(3000)
    );
}

#[test]
fn place_beats_game_beats_global_beats_the_default() {
    let keys = keys();
    let settings = ArgonSettings {
        global: [(Setting::LogLevel, Value::Choice("Info"))].into(),
        game: [(
            "game-a".to_owned(),
            [(Setting::LogLevel, Value::Choice("Debug"))].into(),
        )]
        .into(),
        place: [(
            "place-1".to_owned(),
            [(Setting::LogLevel, Value::Choice("Trace"))].into(),
        )]
        .into(),
    };
    assert_eq!(
        settings.get(Setting::LogLevel, &keys),
        Value::Choice("Trace")
    );
    // Another place of the same game inherits the game's override …
    let other = LevelKeys {
        place: Some("place-2".to_owned()),
        ..keys.clone()
    };
    assert_eq!(
        settings.get(Setting::LogLevel, &other),
        Value::Choice("Debug")
    );
    // … and with no identity at all only Global applies.
    assert_eq!(
        settings.get(Setting::LogLevel, &LevelKeys::default()),
        Value::Choice("Info")
    );
    assert_eq!(settings.get(Setting::AutoConnect, &keys), Value::Bool(true));
}

#[test]
fn overrides_round_trip_through_the_settings_file_shape() {
    let settings = ArgonSettings {
        global: [(Setting::AutoReconnect, Value::Bool(true))].into(),
        game: [(
            "game-a".to_owned(),
            [(Setting::InitialSyncPriority, Value::Choice("Client"))].into(),
        )]
        .into(),
        place: [(
            "place-1".to_owned(),
            [(Setting::ChangesThreshold, Value::Number(12))].into(),
        )]
        .into(),
    };
    let file = serde_json::json!({ "argon": settings.json() });
    assert_eq!(ArgonSettings::read(&file), settings);
    assert_eq!(
        file["argon"]["place"]["place-1"]["ChangesThreshold"],
        serde_json::json!(12)
    );
}

#[test]
fn a_file_from_before_argon_settings_existed_reads_as_all_defaults() {
    let file = serde_json::json!({ "argon_address": "localhost:8000" });
    assert_eq!(ArgonSettings::read(&file), ArgonSettings::default());
}

#[test]
fn malformed_and_unknown_entries_keep_their_defaults() {
    let file = serde_json::json!({ "argon": {
        "global": { "LogLevel": "Loud", "ChangesThreshold": -3, "AutoConnect": "yes", "Nope": 1 },
        "game": { "g": { "OnlyCodeMode": true } },
        "place": "not an object"
    }});
    let settings = ArgonSettings::read(&file);
    // `OnlyCodeMode: true` is the default, so the game entry is empty and pruned.
    assert_eq!(settings, ArgonSettings::default());
}

#[test]
fn overrides_set_at_each_level_resolve_in_order() {
    let mut settings = ArgonSettings::default();
    let keys = keys();
    assert!(settings.set(
        Setting::LogLevel,
        Value::Choice("Info"),
        Level::Global,
        &keys
    ));
    assert_eq!(
        settings.get(Setting::LogLevel, &keys),
        Value::Choice("Info")
    );
    assert!(settings.set(
        Setting::LogLevel,
        Value::Choice("Debug"),
        Level::Game,
        &keys
    ));
    assert_eq!(
        settings.get(Setting::LogLevel, &keys),
        Value::Choice("Debug")
    );
    assert!(settings.set(
        Setting::LogLevel,
        Value::Choice("Trace"),
        Level::Place,
        &keys
    ));
    assert_eq!(
        settings.get(Setting::LogLevel, &keys),
        Value::Choice("Trace")
    );

    // Another place of the same game inherits the game's override.
    let other = LevelKeys {
        place: Some("place-2".to_owned()),
        ..keys.clone()
    };
    assert_eq!(
        settings.get(Setting::LogLevel, &other),
        Value::Choice("Debug")
    );
}
#[test]
fn setting_the_default_removes_the_override_rather_than_storing_it() {
    let mut settings = ArgonSettings::default();
    let keys = keys();
    settings.set(Setting::KeepUnknowns, Value::Bool(true), Level::Game, &keys);
    assert!(settings.set(
        Setting::KeepUnknowns,
        Value::Bool(false),
        Level::Game,
        &keys
    ));
    assert_eq!(settings, ArgonSettings::default());
    // And storing the default where nothing is stored changes nothing.
    assert!(!settings.set(
        Setting::KeepUnknowns,
        Value::Bool(false),
        Level::Game,
        &keys
    ));
}
#[test]
fn a_level_without_a_key_stores_nothing() {
    let mut settings = ArgonSettings::default();
    let keys = LevelKeys::default();
    assert!(!settings.set(
        Setting::OpenInEditor,
        Value::Bool(true),
        Level::Place,
        &keys
    ));
    assert!(!settings.set(Setting::OpenInEditor, Value::Bool(true), Level::Game, &keys));
    assert!(settings.set(
        Setting::OpenInEditor,
        Value::Bool(true),
        Level::Global,
        &keys
    ));
    assert_eq!(
        settings.get(Setting::OpenInEditor, &keys),
        Value::Bool(true)
    );
}
#[test]
fn a_value_of_the_wrong_kind_is_refused() {
    let mut settings = ArgonSettings::default();
    assert!(!settings.set(Setting::LogLevel, Value::Bool(true), Level::Global, &keys()));
    assert!(!settings.set(
        Setting::ChangesThreshold,
        Value::Choice("Warn"),
        Level::Global,
        &keys()
    ));
    assert_eq!(settings, ArgonSettings::default());
}
#[test]
fn restore_defaults_empties_only_the_level_asked_for() {
    let mut settings = ArgonSettings::default();
    let keys = keys();
    settings.set(
        Setting::LiveHydrate,
        Value::Bool(false),
        Level::Global,
        &keys,
    );
    settings.set(
        Setting::LiveHydrate,
        Value::Bool(false),
        Level::Place,
        &keys,
    );
    settings.set(
        Setting::ChangesThreshold,
        Value::Number(20),
        Level::Place,
        &keys,
    );
    assert!(settings.restore_defaults(Level::Place, &keys));
    assert_eq!(
        settings.exact(Setting::LiveHydrate, Level::Place, &keys),
        None
    );
    assert_eq!(
        settings.exact(Setting::ChangesThreshold, Level::Place, &keys),
        None
    );
    assert_eq!(
        settings.exact(Setting::LiveHydrate, Level::Global, &keys),
        Some(Value::Bool(false))
    );
    assert!(!settings.restore_defaults(Level::Place, &keys));
}
