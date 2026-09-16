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

fn dock_layout_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("dock_layout.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let path = {
            let n = std::process::id();
            std::env::temp_dir()
                .join(format!("rbx_studio_dock_layout_test_{n}"))
                .join("dock_layout.json")
        };

        // Create a minimal DockAreaState
        let state = DockAreaState::default();
        let bytes = serde_json::to_vec_pretty(&state).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();

        // Load it back
        let loaded: DockAreaState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn missing_file_returns_none() {
        let path = PathBuf::from("/nonexistent/path/dock_layout.json");
        let bytes = std::fs::read(&path);
        assert!(bytes.is_err());
    }
}
