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

/// Only the manifest fields discovery needs; the full manifest is parsed
/// when the plugin loads.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ManifestHead {
    name: String,
    entry: String,
}

/// rbx-native's own plugins folder: `<config dir>/plugins`
/// (`settings::default_config_dir`), or `RBX_STUDIO_PLUGINS_DIR` when set.
pub(crate) fn own_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("RBX_STUDIO_PLUGINS_DIR") {
        return Some(PathBuf::from(dir));
    }
    crate::settings::default_config_dir().map(|dir| dir.join("plugins"))
}

/// Roblox Studio's Plugins folder, if it exists: `%LOCALAPPDATA%\Roblox\
/// Plugins` on Windows, `~/Documents/Roblox/Plugins` on macOS, and on Linux
/// the Vinegar prefix's (`~/.var/app/org.vinegarhq.Vinegar/data/vinegar/
/// prefixes/studio/drive_c/users/*/AppData/Local/Roblox/Plugins`, then the
/// same under `~/.local/share/vinegar/prefixes/studio`).
pub(crate) fn studio_dir() -> Option<PathBuf> {
    studio_dir_candidates().into_iter().find(|dir| dir.is_dir())
}

#[cfg(windows)]
fn studio_dir_candidates() -> Vec<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| vec![PathBuf::from(local).join("Roblox").join("Plugins")])
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn studio_dir_candidates() -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| {
            vec![PathBuf::from(home)
                .join("Documents")
                .join("Roblox")
                .join("Plugins")]
        })
        .unwrap_or_default()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn studio_dir_candidates() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let flatpak = home.join(".var/app/org.vinegarhq.Vinegar/data/vinegar/prefixes/studio");
    let native = home.join(".local/share/vinegar/prefixes/studio");
    [flatpak, native]
        .iter()
        .flat_map(|prefix| wine_user_plugin_dirs(&prefix.join("drive_c/users")))
        .collect()
}

/// Every `<users>/*/AppData/Local/Roblox/Plugins` under a Wine prefix; the
/// Wine user name is not known ahead of time, so each user directory is
/// tried.
#[cfg(all(unix, not(target_os = "macos")))]
fn wine_user_plugin_dirs(users: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(users) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|user| user.join("AppData/Local/Roblox/Plugins"))
        .collect()
}

/// What is in one folder, sorted by name: plugin files by extension,
/// sub-folders with a `plugin.toml` by their entry's extension (an
/// unreadable manifest still lists the folder as `NativeLuau`, so the
/// error shows when it loads). Anything else is skipped.
pub(crate) fn scan(dir: &Path, from_studio: bool) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Found> = entries
        .flatten()
        .filter_map(|entry| classify(&entry.path(), from_studio))
        .collect();
    found.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    found
}

fn classify(path: &Path, from_studio: bool) -> Option<Found> {
    if path.is_dir() {
        return classify_folder(path, from_studio);
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "lua" | "luau" | "rbxm" | "rbxmx") {
        return None;
    }
    Some(Found {
        path: path.to_path_buf(),
        kind: Kind::Roblox,
        name: path.file_stem()?.to_string_lossy().into_owned(),
        from_studio,
    })
}

fn classify_folder(path: &Path, from_studio: bool) -> Option<Found> {
    let manifest = path.join("plugin.toml");
    if !manifest.is_file() {
        return None;
    }
    let folder_name = path.file_name()?.to_string_lossy().into_owned();
    let head = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| toml::from_str::<ManifestHead>(&text).ok());
    let Some(head) = head else {
        return Some(Found {
            path: path.to_path_buf(),
            kind: Kind::NativeLuau,
            name: folder_name,
            from_studio,
        });
    };
    let entry_ext = Path::new(&head.entry)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let kind = match entry_ext.as_deref() {
        Some("wasm" | "wat") => Kind::NativeWasm,
        _ => Kind::NativeLuau,
    };
    let name = if head.name.trim().is_empty() {
        folder_name
    } else {
        head.name
    };
    Some(Found {
        path: path.to_path_buf(),
        kind,
        name,
        from_studio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("rbx-discover-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scan_classifies_files_and_folders_sorted() {
        let root = TempDir::new("scan");
        let dir = &root.0;
        std::fs::write(dir.join("zeta.rbxmx"), "").unwrap();
        std::fs::write(dir.join("Alpha.LUAU"), "").unwrap();
        std::fs::write(dir.join("beta.lua"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        std::fs::write(dir.join("model.rbxm"), "").unwrap();

        let luau = dir.join("tools");
        std::fs::create_dir_all(&luau).unwrap();
        std::fs::write(
            luau.join("plugin.toml"),
            "name = \"Tools\"\nversion = \"1\"\nentry = \"init.luau\"\n",
        )
        .unwrap();

        let wasm = dir.join("folder_name");
        std::fs::create_dir_all(&wasm).unwrap();
        std::fs::write(wasm.join("plugin.toml"), "entry = \"x.wasm\"\n").unwrap();

        let unrelated = dir.join("assets");
        std::fs::create_dir_all(&unrelated).unwrap();
        std::fs::write(unrelated.join("image.png"), "").unwrap();

        let no_manifest = dir.join("bare");
        std::fs::create_dir_all(&no_manifest).unwrap();

        let found = scan(dir, true);
        let summary: Vec<(String, Kind)> = found
            .iter()
            .map(|f| (f.name.clone(), f.kind.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Alpha".to_owned(), Kind::Roblox),
                ("beta".to_owned(), Kind::Roblox),
                ("folder_name".to_owned(), Kind::NativeWasm),
                ("model".to_owned(), Kind::Roblox),
                ("Tools".to_owned(), Kind::NativeLuau),
                ("zeta".to_owned(), Kind::Roblox),
            ]
        );
        assert!(found.iter().all(|f| f.from_studio));
    }

    #[test]
    fn unreadable_manifest_is_native_luau_under_folder_name() {
        let root = TempDir::new("broken");
        let folder = root.0.join("broken_plugin");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("plugin.toml"), "name = [").unwrap();

        let found = scan(&root.0, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::NativeLuau);
        assert_eq!(found[0].name, "broken_plugin");
        assert!(!found[0].from_studio);
    }

    #[test]
    fn missing_folder_scans_empty() {
        let root = TempDir::new("missing");
        assert!(scan(&root.0.join("nope"), false).is_empty());
    }
}
