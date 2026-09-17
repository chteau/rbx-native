//! Dock panel layout persistence across restarts.
//!
//! Saves and restores the full dock area state (which panels are open/closed,
//! their docked positions, sizes, floating vs. docked) to `dock_layout.json`
//! in the config directory, using `gpui_base::dock::DockAreaState` directly.

use std::path::PathBuf;

use gpui_kit::component::dock::DockAreaState;

use crate::settings::default_config_dir;

/// Load the saved dock layout from disk, falling back to None if the file
/// doesn't exist or is unreadable. Unlike settings, a missing dock layout
/// is not an error — the dock will use its default arrangement.
pub(crate) fn load() -> Option<DockAreaState> {
    let path = dock_layout_path()?;
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Save the current dock layout to disk. Errors are silently dropped since
/// a lost layout preference is not fatal.
pub(crate) fn save(state: &DockAreaState) {
    let Some(path) = dock_layout_path() else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec_pretty(state) else {
        return;
    };
    use crate::settings::write_atomic;
    let _ = write_atomic(&path, &bytes);
}

/// Versioned by filename: a saved layout names the panels it arranges, so one
/// written before a panel existed would silently hide that panel forever
/// rather than fail. Bumping the name discards such a layout instead, which
/// costs a rearrangement once and never loses a panel.
fn dock_layout_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("dock_layout_v3.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_config_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rbx_studio_dock_test_{}_{n}", std::process::id()))
    }

    #[test]
    fn save_and_load_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let state = DockAreaState::default();
        save(&state);

        let loaded = load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn missing_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        // Don't create the directory, so load() returns None
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let loaded = load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded, None);
    }

    #[test]
    fn malformed_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        std::fs::create_dir_all(dir.join("rbx-native")).unwrap();
        std::fs::write(
            dir.join("rbx-native/dock_layout_v3.json"),
            b"not valid json at all {{{",
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let loaded = load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded, None);
    }
}
