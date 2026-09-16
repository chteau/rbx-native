//! Serializable dock panel layout state for persistence across restarts.
//!
//! The dock area's layout (which panels are visible, how they're arranged, their
//! sizes and positions) is saved and restored from disk, so the editor opens with
//! the same arrangement the user left it in.

use std::path::{Path, PathBuf};

/// Which of Shell's sections a panel represents — mirrors the names used by
/// the panel registry in `shell::dock` (must stay in sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum PanelKind {
    Viewport,
    Explorer,
    Properties,
    Output,
}

impl PanelKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Viewport => "Viewport",
            Self::Explorer => "Explorer",
            Self::Properties => "Properties",
            Self::Output => "Output",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "Viewport" => Some(Self::Viewport),
            "Explorer" => Some(Self::Explorer),
            "Properties" => Some(Self::Properties),
            "Output" => Some(Self::Output),
            _ => None,
        }
    }
}

/// A single panel's visibility and sizing state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PanelState {
    pub(crate) kind: PanelKind,
    /// Whether this panel is currently visible (opened vs. closed).
    pub(crate) visible: bool,
}

/// The complete dock layout state — which panels are visible, their relative
/// sizes, arrangement, etc. Saved and restored to preserve the editor's layout
/// across restarts.
///
/// This is a simple, flat list of panels for now. A more sophisticated
/// representation could capture the full tree structure (splits, tabs, floating
/// windows, etc.), but that's deferred — this baseline captures the most
/// essential state: which panels the user has open.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DockLayoutState {
    pub(crate) panels: Vec<PanelState>,
}

impl Default for DockLayoutState {
    /// The default state matches the hardcoded dock layout in `shell::dock::build`:
    /// all panels visible.
    fn default() -> Self {
        DockLayoutState {
            panels: vec![
                PanelState {
                    kind: PanelKind::Viewport,
                    visible: true,
                },
                PanelState {
                    kind: PanelKind::Output,
                    visible: true,
                },
                PanelState {
                    kind: PanelKind::Explorer,
                    visible: true,
                },
                PanelState {
                    kind: PanelKind::Properties,
                    visible: true,
                },
            ],
        }
    }
}

impl DockLayoutState {
    /// Reads `$XDG_CONFIG_HOME/rbx-native/dock_layout.json` (or equivalent on
    /// other platforms, following the same directory convention as
    /// `settings.json`). Returns the default layout if the file is missing,
    /// malformed, or unreadable — never fails.
    pub(crate) fn load() -> Self {
        match default_dock_layout_path() {
            Some(path) => load_from(&path),
            None => Self::default(),
        }
    }

    /// Writes the dock layout to `$XDG_CONFIG_HOME/rbx-native/dock_layout.json`
    /// (or equivalent on other platforms). Returns an error if the directory
    /// cannot be created or the file cannot be written; a missing config
    /// directory is not fatal and silently falls back (same as `Settings`).
    pub(crate) fn save(&self) -> Result<(), DockLayoutError> {
        let path = default_dock_layout_path().ok_or(DockLayoutError::NoConfigDir)?;
        save_to(self, &path)
    }
}

/// Errors saving the dock layout file. Loading never errors (see
/// [`DockLayoutState::load`]); this exists only for the write side, and callers are
/// free to ignore it since a lost layout is not fatal.
#[derive(Debug)]
pub(crate) enum DockLayoutError {
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

impl std::fmt::Display for DockLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoConfigDir => {
                write!(
                    f,
                    "could not determine a config directory (no XDG_CONFIG_HOME, \
                     APPDATA, or HOME)"
                )
            }
            Self::CreateDir { path, source } => {
                write!(
                    f,
                    "failed to create config directory {}: {source}",
                    path.display()
                )
            }
            Self::Write { path, source } => {
                write!(
                    f,
                    "failed to write dock layout file {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for DockLayoutError {}

fn load_from(path: &Path) -> DockLayoutState {
    let Ok(bytes) = std::fs::read(path) else {
        return DockLayoutState::default();
    };
    let Ok(state) = serde_json::from_slice::<DockLayoutState>(&bytes) else {
        return DockLayoutState::default();
    };
    state
}

fn save_to(layout: &DockLayoutState, path: &Path) -> Result<(), DockLayoutError> {
    let bytes = serde_json::to_vec_pretty(layout)
        .expect("dock layout always serializes");
    write_atomic(path, &bytes)
}

fn default_dock_layout_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("dock_layout.json"))
}

fn default_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("rbx-native"));
    }
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DockLayoutError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| DockLayoutError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, bytes).map_err(|source| DockLayoutError::Write {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|source| DockLayoutError::Write {
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
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dock_layout_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "rbx_studio_dock_layout_test_{}_{n}",
                std::process::id()
            ))
            .join("dock_layout.json")
    }

    #[test]
    fn default_layout_has_all_panels_visible() {
        let layout = DockLayoutState::default();
        assert_eq!(layout.panels.len(), 4);
        assert!(layout
            .panels
            .iter()
            .all(|p| p.visible == true));
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_dock_layout_path();
        let layout = DockLayoutState {
            panels: vec![
                PanelState {
                    kind: PanelKind::Viewport,
                    visible: true,
                },
                PanelState {
                    kind: PanelKind::Output,
                    visible: false,
                },
                PanelState {
                    kind: PanelKind::Explorer,
                    visible: true,
                },
                PanelState {
                    kind: PanelKind::Properties,
                    visible: true,
                },
            ],
        };
        save_to(&layout, &path).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.panels.len(), layout.panels.len());
        for (saved, loaded) in layout.panels.iter().zip(loaded.panels.iter()) {
            assert_eq!(saved.kind, loaded.kind);
            assert_eq!(saved.visible, loaded.visible);
        }
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_dock_layout_path();
        let layout = load_from(&path);
        assert_eq!(layout, DockLayoutState::default());
    }

    #[test]
    fn malformed_json_falls_back_to_defaults_instead_of_crashing() {
        let path = temp_dock_layout_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json at all {{{").unwrap();
        let layout = load_from(&path);
        assert_eq!(layout, DockLayoutState::default());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_dock_layout_path()
            .parent()
            .unwrap()
            .join("nested")
            .join("deep")
            .join("dock_layout.json");
        let layout = DockLayoutState::default();
        save_to(&layout, &path).unwrap();
        assert!(path.is_file());
        let loaded = load_from(&path);
        assert_eq!(loaded, layout);
    }

    #[test]
    fn panel_kind_round_trips_through_string() {
        let kinds = [
            PanelKind::Viewport,
            PanelKind::Explorer,
            PanelKind::Properties,
            PanelKind::Output,
        ];
        for kind in kinds {
            assert_eq!(PanelKind::from_str(kind.as_str()), Some(kind));
        }
    }
}
