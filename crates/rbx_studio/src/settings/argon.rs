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

impl Value {
    fn same_kind(&self, other: &Value) -> bool {
        matches!(
            (self, other),
            (Value::Bool(_), Value::Bool(_))
                | (Value::Choice(_), Value::Choice(_))
                | (Value::Number(_), Value::Number(_))
        )
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

    pub(crate) fn label(self) -> &'static str {
        match self {
            Level::Place => "Place",
            Level::Game => "Game",
            Level::Global => "Global",
        }
    }
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
        Level::ALL
            .iter()
            .find_map(|level| self.overrides(*level, keys)?.get(&setting).cloned())
            .unwrap_or_else(|| setting.default())
    }

    /// The override stored at exactly `level`, if any — the plugin's
    /// `Config:get(setting, level, true)` (`Config.luau:91-93`).
    pub(crate) fn exact(&self, setting: Setting, level: Level, keys: &LevelKeys) -> Option<Value> {
        self.overrides(level, keys)?.get(&setting).cloned()
    }

    /// Stores `value` at `level`, dropping the override instead when it is
    /// the default (`Config.luau:147`). A `value` of the wrong kind for the
    /// setting, or a level with no key, changes nothing. Returns whether
    /// anything changed.
    pub(crate) fn set(
        &mut self,
        setting: Setting,
        value: Value,
        level: Level,
        keys: &LevelKeys,
    ) -> bool {
        if !value.same_kind(&setting.default()) {
            return false;
        }
        let overrides = match level {
            Level::Global => &mut self.global,
            Level::Game => match &keys.game {
                Some(key) => self.game.entry(key.clone()).or_default(),
                None => return false,
            },
            Level::Place => match &keys.place {
                Some(key) => self.place.entry(key.clone()).or_default(),
                None => return false,
            },
        };
        let before = overrides.get(&setting).cloned();
        if value == setting.default() {
            overrides.remove(&setting);
        } else {
            overrides.insert(setting, value);
        }
        let changed = before != overrides.get(&setting).cloned();
        self.prune();
        changed
    }

    /// What `level` falls back to for `setting`: the nearest level above
    /// it that overrides it, else the default, reported as Global since
    /// that is where a default is changed.
    pub(crate) fn inherited(
        &self,
        setting: Setting,
        level: Level,
        keys: &LevelKeys,
    ) -> (Level, Value) {
        Level::ALL
            .iter()
            .skip_while(|above| **above != level)
            .skip(1)
            .find_map(|above| Some((*above, self.overrides(*above, keys)?.get(&setting)?.clone())))
            .unwrap_or_else(|| (Level::Global, setting.default()))
    }

    /// Drops `level`'s own override of `setting`, so it falls back to what
    /// it inherits. Returns whether there was one.
    pub(crate) fn clear(&mut self, setting: Setting, level: Level, keys: &LevelKeys) -> bool {
        let removed = match level {
            Level::Global => self.global.remove(&setting).is_some(),
            Level::Game => keys
                .game
                .as_ref()
                .and_then(|key| self.game.get_mut(key)?.remove(&setting))
                .is_some(),
            Level::Place => keys
                .place
                .as_ref()
                .and_then(|key| self.place.get_mut(key)?.remove(&setting))
                .is_some(),
        };
        self.prune();
        removed
    }

    /// Empties one level (`Config.luau:162-174`). Returns whether it held
    /// anything.
    pub(crate) fn restore_defaults(&mut self, level: Level, keys: &LevelKeys) -> bool {
        let removed = match level {
            Level::Global => !std::mem::take(&mut self.global).is_empty(),
            Level::Game => keys
                .game
                .as_ref()
                .and_then(|key| self.game.remove(key))
                .is_some_and(|overrides| !overrides.is_empty()),
            Level::Place => keys
                .place
                .as_ref()
                .and_then(|key| self.place.remove(key))
                .is_some_and(|overrides| !overrides.is_empty()),
        };
        self.prune();
        removed
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
mod tests;
