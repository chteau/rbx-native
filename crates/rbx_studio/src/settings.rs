//! Persisted Studio preferences: the graphics quality dropdown, the
//! Explorer's "show all services" checkbox, and the Viewport's orthographic
//! toggle, so a relaunch reopens where the user left off rather than always
//! at the hardcoded defaults.
//!
//! Mirrors `rbx_assets::AssetCache`'s directory convention (`$XDG_CONFIG_HOME`,
//! falling back to `~/.config` or, on Windows, `%APPDATA%`, all under an
//! `rbx-native` folder) and its temp-file-then-rename write, but for one small
//! config document rather than many cached blobs, and `config` rather than
//! `cache` — a setting should survive `rm -rf ~/.cache`.

use std::path::{Path, PathBuf};

use rbx_viewer::QualityLevel;

/// What persists across a relaunch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) quality: QualityLevel,
    pub(crate) show_all_services: bool,
    pub(crate) orthographic: bool,
}

impl Default for Settings {
    /// Same defaults `Shell`/`main` used before either was configurable, so a
    /// missing settings file changes nothing about a first run.
    fn default() -> Self {
        Settings {
            quality: QualityLevel::Automatic,
            show_all_services: false,
            orthographic: false,
        }
    }
}

impl Settings {
    /// Reads `$XDG_CONFIG_HOME/rbx-native/settings.json` (or
    /// `~/.config/rbx-native/settings.json`, or `%APPDATA%\rbx-native\
    /// settings.json` on Windows). Never fails: a missing file, a malformed
    /// document, an unreadable path or an undeterminable config directory all
    /// fall back to [`Settings::default`] rather than blocking or aborting
    /// startup.
    pub(crate) fn load() -> Settings {
        match default_settings_path() {
            Some(path) => load_from(&path),
            None => Settings::default(),
        }
    }

    /// Writes the same path `load` reads, creating the directory if needed
    /// and replacing the file atomically. Cheap enough to call on every
    /// change rather than debouncing.
    pub(crate) fn save(&self) -> Result<(), SettingsError> {
        let path = default_settings_path().ok_or(SettingsError::NoConfigDir)?;
        save_to(self, &path)
    }
}

/// Errors saving the settings file. Loading never errors (see
/// [`Settings::load`]); this exists only for the write side, and callers are
/// free to ignore it since a lost preference is not fatal.
#[derive(Debug)]
pub(crate) enum SettingsError {
    NoConfigDir,
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::NoConfigDir => {
                write!(
                    f,
                    "could not determine a config directory (no XDG_CONFIG_HOME, \
                     APPDATA, or HOME)"
                )
            }
            SettingsError::CreateDir { path, source } => {
                write!(
                    f,
                    "failed to create config directory {}: {source}",
                    path.display()
                )
            }
            SettingsError::Write { path, source } => {
                write!(
                    f,
                    "failed to write settings file {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {}

fn load_from(path: &Path) -> Settings {
    let Ok(bytes) = std::fs::read(path) else {
        return Settings::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Settings::default();
    };

    let quality = value
        .get("quality")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<QualityLevel>().ok())
        .unwrap_or(QualityLevel::Automatic);
    let show_all_services = value
        .get("show_all_services")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let orthographic = value
        .get("orthographic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Settings {
        quality,
        show_all_services,
        orthographic,
    }
}

fn save_to(settings: &Settings, path: &Path) -> Result<(), SettingsError> {
    let value = serde_json::json!({
        "quality": format_quality(settings.quality),
        "show_all_services": settings.show_all_services,
        "orthographic": settings.orthographic,
    });
    // A two-field object always serializes; nothing here can fail.
    let bytes = serde_json::to_vec_pretty(&value).expect("settings JSON always serializes");
    write_atomic(path, &bytes)
}

/// Roblox's own `Enum.QualityLevel` spelling, the same format `Shell`'s
/// dropdown labels and command line already parse back with
/// [`QualityLevel::from_str`] — `QualityLevel` has no `Display` impl of its
/// own to reuse.
fn format_quality(quality: QualityLevel) -> String {
    match quality {
        QualityLevel::Automatic => "Automatic".to_string(),
        QualityLevel::Level(level) => format!("Level{level:02}"),
    }
}

fn default_settings_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("settings.json"))
}

fn default_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("rbx-native"));
    }
    // %APPDATA% is Windows' roaming per-user data dir, the natural match for
    // a small settings file (unlike %LOCALAPPDATA%, used for the asset
    // cache — see rbx_assets::AssetCache). Checked before HOME so a
    // Windows-native launch never depends on HOME being set.
    if let Some(appdata) = non_empty_env("APPDATA") {
        return Some(PathBuf::from(appdata).join("rbx-native"));
    }
    if let Some(home) = non_empty_env("HOME") {
        return Some(PathBuf::from(home).join(".config").join("rbx-native"));
    }
    None
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Writes via a temp file + rename so a reader never observes a partially
/// written settings file, and a crash mid-write can't corrupt an existing one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| SettingsError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, bytes).map_err(|source| SettingsError::Write {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    /// Guards every test below that mutates `XDG_CONFIG_HOME`/`APPDATA`: env
    /// vars are process-global, and `cargo test` runs tests on separate
    /// threads of the same process, so two such tests running concurrently
    /// would each observe the other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A settings.json path under a fresh, never-created temp directory: each
    /// test gets its own, so tests never share (or need to clean up) state,
    /// and the real `$XDG_CONFIG_HOME`/`HOME` are never touched.
    fn temp_settings_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "rbx_studio_settings_test_{}_{n}",
                std::process::id()
            ))
            .join("settings.json")
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_settings_path();
        let settings = Settings {
            quality: QualityLevel::Level(7),
            show_all_services: true,
            orthographic: true,
        };
        save_to(&settings, &path).unwrap();
        assert_eq!(load_from(&path), settings);
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_settings_path();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn malformed_json_falls_back_to_defaults_instead_of_crashing() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json at all {{{").unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn unreadable_path_falls_back_to_defaults() {
        // A directory where a file is expected can never be `fs::read`.
        let path = temp_settings_path();
        std::fs::create_dir_all(&path).unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_settings_path()
            .parent()
            .unwrap()
            .join("nested")
            .join("deep")
            .join("settings.json");
        let settings = Settings::default();
        save_to(&settings, &path).unwrap();
        assert!(path.is_file());
        assert_eq!(load_from(&path), settings);
    }

    #[test]
    fn default_dir_honors_xdg_config_home_over_the_home_fallback() {
        // SAFETY: env var mutation is process-global; guarded by ENV_LOCK so
        // this can't interleave with the other tests below that also mutate
        // XDG_CONFIG_HOME/APPDATA/HOME — scope the check to the
        // pure-computation helper rather than asserting on the process-wide
        // environment anywhere else.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let result = default_config_dir();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(result, Some(dir.join("rbx-native")));
    }

    #[test]
    fn default_dir_falls_back_to_appdata_on_windows() {
        // SAFETY: see default_dir_honors_xdg_config_home_over_the_home_fallback.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("APPDATA", &dir);
        let result = default_config_dir();
        std::env::remove_var("APPDATA");
        assert_eq!(result, Some(dir.join("rbx-native")));
    }

    #[test]
    fn xdg_config_home_wins_over_appdata() {
        // SAFETY: see default_dir_honors_xdg_config_home_over_the_home_fallback.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let xdg = temp_settings_path().parent().unwrap().to_path_buf();
        let appdata = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        std::env::set_var("APPDATA", &appdata);
        let result = default_config_dir();
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("APPDATA");
        assert_eq!(result, Some(xdg.join("rbx-native")));
    }

    #[test]
    fn quality_string_form_round_trips_every_level_and_automatic() {
        assert_eq!(
            format_quality(QualityLevel::Automatic).parse::<QualityLevel>(),
            Ok(QualityLevel::Automatic)
        );
        for level in QualityLevel::MIN..=QualityLevel::MAX {
            let mode = QualityLevel::Level(level);
            assert_eq!(format_quality(mode).parse::<QualityLevel>(), Ok(mode));
        }
    }
}
