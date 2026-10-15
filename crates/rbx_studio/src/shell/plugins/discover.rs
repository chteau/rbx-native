//! Where plugins are installed and what each file or folder is.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A `.lua`/`.luau`/`.rbxm`/`.rbxmx` Roblox Studio plugin.
    Roblox,
    /// A folder with a `plugin.toml` whose entry is Luau.
    NativeLuau,
    /// A folder with a `plugin.toml` whose entry is `.wasm`/`.wat`.
    NativeWasm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) path: PathBuf,
    pub(crate) kind: Kind,
    /// File stem, or the manifest's `name`.
    pub(crate) name: String,
    /// Found in Roblox Studio's own Plugins folder.
    pub(crate) from_studio: bool,
}

/// rbx-native's own plugins folder: `<config dir>/plugins`
/// (`settings::default_config_dir`), or `RBX_STUDIO_PLUGINS_DIR` when set.
pub(crate) fn own_dir() -> Option<PathBuf> {
    todo!()
}

/// Roblox Studio's Plugins folder, if it exists: `%LOCALAPPDATA%\Roblox\
/// Plugins` on Windows, `~/Documents/Roblox/Plugins` on macOS, and on Linux
/// the Vinegar prefix's (`~/.var/app/org.vinegarhq.Vinegar/data/vinegar/
/// prefixes/studio/drive_c/users/*/AppData/Local/Roblox/Plugins`, then the
/// same under `~/.local/share/vinegar/prefixes/studio`).
pub(crate) fn studio_dir() -> Option<PathBuf> {
    todo!()
}

/// What is in one folder, sorted by name: plugin files by extension,
/// sub-folders with a `plugin.toml` by their entry's extension (an
/// unreadable manifest still lists the folder as `NativeLuau`, so the
/// error shows when it loads). Anything else is skipped.
pub(crate) fn scan(dir: &Path, from_studio: bool) -> Vec<Found> {
    todo!()
}
