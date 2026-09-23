//! Argon's own plugin settings, ported from its Roblox Studio plugin
//! (`argon-rbx/argon-roblox@30fd38d`, `src/Config.luau`) so that the Argon
//! dock behaves the way the plugin does.
//!
//! The plugin keeps three levels of overrides — Place, Game and Global —
//! and resolves a setting by walking them in that order, falling back to
//! the setting's default (`Config.luau:105-121`). An override equal to the
//! default is not stored at all (`:147`), and restoring defaults empties
//! one level (`:162-174`). Server host and port are not settings here:
//! the dock's address field already remembers them (`Settings::argon_address`).
//!
//! What identifies a Game or a Place is the plugin's `game.GameId` /
//! `game.PlaceId` (`Config.luau:61-62`), which a local place file doesn't
//! have. This module takes the two keys as opaque strings ([`LevelKeys`])
//! and leaves what they identify to the caller.
//!
//! Stored as one `"argon"` object in the settings file: `"global"` holds
//! the Global overrides, `"game"` and `"place"` map each key to its own.

use std::collections::BTreeMap;

/// One of the plugin's settings, named as `Config.luau:9-26` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Setting {
    AutoConnect,
    AutoReconnect,
    Https,
    InitialSyncPriority,
    LiveHydrate,
    KeepUnknowns,
    OverridePackages,
    TwoWaySync,
    SyncbackProperties,
    OnlyCodeMode,
    DisplayPrompts,
    ChangesThreshold,
    DiffLinesLimit,
    OpenInEditor,
    LogLevel,
}

/// A setting's value: the three kinds the plugin's settings come in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Bool(bool),
    /// One of [`Setting::choices`], for the dropdown settings.
    Choice(&'static str),
    Number(u32),
}

impl Setting {
    pub(crate) const ALL: [Setting; 15] = [
        Setting::AutoConnect,
        Setting::AutoReconnect,
        Setting::Https,
        Setting::InitialSyncPriority,
        Setting::LiveHydrate,
        Setting::KeepUnknowns,
        Setting::OverridePackages,
        Setting::TwoWaySync,
        Setting::SyncbackProperties,
        Setting::OnlyCodeMode,
        Setting::DisplayPrompts,
        Setting::ChangesThreshold,
        Setting::DiffLinesLimit,
        Setting::OpenInEditor,
        Setting::LogLevel,
    ];

    /// The plugin's own name for the setting, which is also its key in the
    /// settings file.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Setting::AutoConnect => "AutoConnect",
            Setting::AutoReconnect => "AutoReconnect",
            Setting::Https => "Https",
            Setting::InitialSyncPriority => "InitialSyncPriority",
            Setting::LiveHydrate => "LiveHydrate",
            Setting::KeepUnknowns => "KeepUnknowns",
            Setting::OverridePackages => "OverridePackages",
            Setting::TwoWaySync => "TwoWaySync",
            Setting::SyncbackProperties => "SyncbackProperties",
            Setting::OnlyCodeMode => "OnlyCodeMode",
            Setting::DisplayPrompts => "DisplayPrompts",
            Setting::ChangesThreshold => "ChangesThreshold",
            Setting::DiffLinesLimit => "DiffLinesLimit",
            Setting::OpenInEditor => "OpenInEditor",
            Setting::LogLevel => "LogLevel",
        }
    }

    /// The plugin's defaults, `Config.luau:39-57`.
    pub(crate) fn default(self) -> Value {
        match self {
            Setting::AutoConnect => Value::Bool(true),
            Setting::AutoReconnect => Value::Bool(false),
            Setting::Https => Value::Bool(false),
            Setting::InitialSyncPriority => Value::Choice("Server"),
            Setting::LiveHydrate => Value::Bool(true),
            Setting::KeepUnknowns => Value::Bool(false),
            Setting::OverridePackages => Value::Bool(true),
            Setting::TwoWaySync => Value::Bool(false),
            Setting::SyncbackProperties => Value::Bool(false),
            Setting::OnlyCodeMode => Value::Bool(true),
            Setting::DisplayPrompts => Value::Choice("Always"),
            Setting::ChangesThreshold => Value::Number(5),
            Setting::DiffLinesLimit => Value::Number(3000),
            Setting::OpenInEditor => Value::Bool(false),
            Setting::LogLevel => Value::Choice("Warn"),
        }
    }

    /// The dropdown options, in the plugin's order (`Settings.luau:54`,
    /// `:141`, `:154`). Empty for a switch or a number.
    pub(crate) fn choices(self) -> &'static [&'static str] {
        match self {
            Setting::InitialSyncPriority => &["Server", "Client", "None"],
            Setting::DisplayPrompts => &["Always", "Initial", "Never"],
            Setting::LogLevel => &["Off", "Error", "Warn", "Info", "Debug", "Trace"],
            _ => &[],
        }
    }

    /// A stored value, if it is the kind this setting takes — the plugin
    /// casts to the default's type (`Config.luau:146`); anything that
    /// doesn't cast keeps the default.
    fn parse(self, json: &serde_json::Value) -> Option<Value> {
        match self.default() {
            Value::Bool(_) => json.as_bool().map(Value::Bool),
            Value::Number(_) => json
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .map(Value::Number),
            Value::Choice(_) => {
                let text = json.as_str()?;
                self.choices()
                    .iter()
                    .find(|choice| **choice == text)
                    .map(|choice| Value::Choice(choice))
            }
        }
    }
}

impl Value {
    fn json(&self) -> serde_json::Value {
        match self {
            Value::Bool(on) => serde_json::Value::Bool(*on),
            Value::Choice(choice) => serde_json::Value::String((*choice).to_owned()),
            Value::Number(n) => serde_json::Value::from(*n),
        }
    }
}

/// The plugin's three configuration levels, `Config.luau:8`, in the
/// order they are resolved (`:59`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Place,
    Game,
    Global,
}

impl Level {
    pub(crate) const ALL: [Level; 3] = [Level::Place, Level::Game, Level::Global];
}

/// What identifies the open place's Game and Place levels. `None` means
/// the level has no identity (nothing connected, an unpublished project),
/// and then it holds no overrides and stores none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LevelKeys {
    pub(crate) game: Option<String>,
    pub(crate) place: Option<String>,
}

/// The overrides at one level: only the settings that differ from their
/// default, as the plugin stores them.
type Overrides = BTreeMap<Setting, Value>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ArgonSettings {
    global: Overrides,
    game: BTreeMap<String, Overrides>,
    place: BTreeMap<String, Overrides>,
}

impl ArgonSettings {
    fn overrides(&self, level: Level, keys: &LevelKeys) -> Option<&Overrides> {
        match level {
            Level::Global => Some(&self.global),
            Level::Game => self.game.get(keys.game.as_ref()?),
            Level::Place => self.place.get(keys.place.as_ref()?),
        }
    }

    /// The value in force: Place, then Game, then Global, then the default
    /// (`Config.luau:105-121` with no level).
    pub(crate) fn get(&self, setting: Setting, keys: &LevelKeys) -> Value {
        self.inherited_from(setting, 0, keys)
    }

    fn inherited_from(&self, setting: Setting, start: usize, keys: &LevelKeys) -> Value {
        Level::ALL[start..]
            .iter()
            .find_map(|level| self.overrides(*level, keys)?.get(&setting).cloned())
            .unwrap_or_else(|| setting.default())
    }

    /// A key whose overrides are all gone is not worth a line in the file
    /// — the plugin deletes the level's setting outright then
    /// (`Config.luau:155-156`).
    fn prune(&mut self) {
        self.game.retain(|_, overrides| !overrides.is_empty());
        self.place.retain(|_, overrides| !overrides.is_empty());
    }

    pub(super) fn read(value: &serde_json::Value) -> ArgonSettings {
        let stored = value.get("argon");
        let level = |name: &str| stored.and_then(|stored| stored.get(name));
        let keyed = |name: &str| -> BTreeMap<String, Overrides> {
            level(name)
                .and_then(serde_json::Value::as_object)
                .map(|map| {
                    map.iter()
                        .map(|(key, overrides)| (key.clone(), read_overrides(overrides)))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut settings = ArgonSettings {
            global: level("global").map(read_overrides).unwrap_or_default(),
            game: keyed("game"),
            place: keyed("place"),
        };
        settings.prune();
        settings
    }

    pub(super) fn json(&self) -> serde_json::Value {
        let keyed = |map: &BTreeMap<String, Overrides>| -> serde_json::Value {
            serde_json::Value::Object(
                map.iter()
                    .map(|(key, overrides)| (key.clone(), overrides_json(overrides)))
                    .collect(),
            )
        };
        serde_json::json!({
            "global": overrides_json(&self.global),
            "game": keyed(&self.game),
            "place": keyed(&self.place),
        })
    }
}

/// One level's stored object. A value that isn't the setting's kind, or a
/// key that isn't a setting, is skipped; a value equal to the default is
/// dropped, since it would never have been written.
fn read_overrides(value: &serde_json::Value) -> Overrides {
    Setting::ALL
        .into_iter()
        .filter_map(|setting| {
            let parsed = setting.parse(value.get(setting.key())?)?;
            (parsed != setting.default()).then_some((setting, parsed))
        })
        .collect()
}

fn overrides_json(overrides: &Overrides) -> serde_json::Value {
    serde_json::Value::Object(
        overrides
            .iter()
            .map(|(setting, value)| (setting.key().to_owned(), value.json()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
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
}
