//! A place's Local Run data: its DataStores and what its test players
//! bought, kept between runs in `<config>/local-run/<place>/`, and how the
//! next run treats them (Test › Data), and the place files a teleport may
//! reload into.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Test › Data, per place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DataMode {
    /// DataStores are kept from one run to the next.
    #[default]
    Kept,
    /// Every run starts with empty DataStores, forgotten when it stops.
    Fresh,
    /// "Enable Studio Access to API Services" off: DataStore calls fail as
    /// Studio's do.
    Off,
}

impl DataMode {
    pub(crate) fn next(self) -> DataMode {
        match self {
            DataMode::Kept => DataMode::Fresh,
            DataMode::Fresh => DataMode::Off,
            DataMode::Off => DataMode::Kept,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            DataMode::Kept => "Data: Kept",
            DataMode::Fresh => "Data: Fresh",
            DataMode::Off => "Data: Off",
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    data: DataMode,
    /// Place files a teleport to their place id reloads into, besides the
    /// place itself (TeleportService in Local Run). Edited in the place's
    /// `settings.json`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    teleport_places: BTreeMap<i64, PathBuf>,
}

fn settings(path: &Path) -> Settings {
    folder(path)
        .and_then(|dir| std::fs::read(dir.join("settings.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default()
}

/// The place file a teleport to `place` reloads into, if one's mapped.
pub(crate) fn teleport_place(path: &Path, place: i64) -> Option<PathBuf> {
    settings(path).teleport_places.get(&place).cloned()
}

/// The place's folder: its published place id when it's linked, else a
/// hash of where the file is.
pub(crate) fn folder(path: &Path) -> Option<PathBuf> {
    let key = match crate::home::link_of(path) {
        Some(link) => format!("place-{}", link.place_id),
        None => {
            let full = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let digest = Sha256::digest(full.to_string_lossy().as_bytes());
            let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
            format!("file-{hex}")
        }
    };
    Some(
        crate::settings::default_config_dir()?
            .join("local-run")
            .join(key),
    )
}

pub(crate) fn mode(path: &Path) -> DataMode {
    settings(path).data
}

pub(crate) fn set_mode(path: &Path, data: DataMode) {
    let Some(dir) = folder(path) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let settings = Settings {
        data,
        ..settings(path)
    };
    if let Ok(text) = serde_json::to_vec_pretty(&settings) {
        let _ = std::fs::write(dir.join("settings.json"), text);
    }
}

/// What the run's scripts get for `path` in `mode`.
pub(crate) fn services(path: &Path, mode: DataMode) -> rbx_lua::server::Services {
    let dir = folder(path);
    let link = crate::home::link_of(path);
    rbx_lua::server::Services {
        universe: dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
        secrets: dir
            .as_ref()
            .map(|d| secrets(&d.join("secrets.json")))
            .unwrap_or_default(),
        data_file: match mode {
            DataMode::Fresh => None,
            _ => dir.map(|d| d.join("datastores.json")),
        },
        api_access: mode != DataMode::Off,
        cache_dir: rbx_assets::cache_root().map(|root| root.join("local-run")),
        teleport_places: settings(path).teleport_places.into_keys().collect(),
        place_id: link.as_ref().map_or(0, |l| l.place_id as i64),
        game_id: link.as_ref().map_or(0, |l| l.universe_id as i64),
        web_lookups: true,
        ..Default::default()
    }
}

/// Studio's local secrets for `HttpService:GetSecret`, in the format
/// Studio's Game Settings take them: `{"name": ["value", "domain"]}`.
fn secrets(file: &Path) -> std::collections::HashMap<String, (String, String)> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Connects the run to the experience's real DataStores, as Studio's Play
/// has them, when the place is published, API access is on and the editor
/// is online with an Open Cloud key; else they stay in the local store.
/// The one line Output shows either way. `RBXNATIVE_LOCAL_RUN_READONLY=1`
/// keeps every live write in the run, never sent (for testing against games that must not
/// change).
pub(crate) fn go_live(
    path: &Path,
    mode: DataMode,
    services: &mut rbx_lua::server::Services,
) -> String {
    let local = |why: &str| format!("DataStores: local ({why})");
    if mode == DataMode::Off {
        return local("Studio access to API services is off");
    }
    let Some(link) = crate::home::link_of(path) else {
        return local("the place isn\u{2019}t published");
    };
    let read_only = std::env::var("RBXNATIVE_LOCAL_RUN_READONLY").is_ok_and(|v| v == "1");
    match rbx_sim::live::connect(link.universe_id, read_only) {
        Ok(live) => {
            services.live = Some(live);
            format!(
                "DataStores: live, experience {}{}",
                link.universe_id,
                if read_only { " (read-only)" } else { "" }
            )
        }
        Err(why) => local(&why.reason()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_cycle_and_off_turns_api_access_off() {
        assert_eq!(DataMode::Kept.next().next().next(), DataMode::Kept);
        let path = Path::new("/nowhere/place.rbxl");
        assert!(!services(path, DataMode::Off).api_access);
        assert!(services(path, DataMode::Fresh).data_file.is_none());
        let mut config = services(path, DataMode::Kept);
        assert!(go_live(path, DataMode::Kept, &mut config).contains("isn\u{2019}t published"));
        assert!(config.live.is_none());
    }

    #[test]
    fn local_secrets_read_studio_s_format() {
        let dir = std::env::temp_dir().join(format!("rbxnative-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("secrets.json");
        std::fs::write(&file, r#"{"key": ["abc", "*.example.com"]}"#).unwrap();
        let found = secrets(&file);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found["key"], ("abc".into(), "*.example.com".into()));
        assert!(secrets(Path::new("/nowhere/secrets.json")).is_empty());
    }
}
