//! Per-place Explorer colour tags for `Folder` rows — a purely local editor
//! convenience with no corresponding real Roblox property, so it lives here
//! (`$XDG_CONFIG_HOME/rbx-native/folder_colors.json`, mirroring `settings.rs`'s
//! directory convention) rather than in the saved place file. See
//! `ROADMAP.md`'s "Colour-coded Explorer folders" for why: a real Studio
//! session opening the same place must never see an invented `Folder`
//! property.
//!
//! Keyed by the place's own file path, then by the tagged folder's Explorer
//! path (`Workspace.Nested.MyFolder`, see [`path_of`]) rather than its `Ref`
//! — a `Ref` regenerates on every load, so it can never be a stable key
//! across a reload.
//!
//! ponytail: path-keyed, so renaming or moving a tagged folder orphans its
//! entry — the old path no longer resolves, so the tint silently stops
//! applying, and [`FolderColors::prune`] is the one-line upkeep that clears
//! the orphan rather than leaving it to grow forever. Upgrade to a real
//! content-addressed key (a `UniqueId`-derived id, if `Folder` reliably
//! carries one, or once the DOM gets a stable id concept of its own) if a
//! tag ever needs to survive a rename.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rbx_dom::{Ref, WeakDom};
use serde::{Deserialize, Serialize};

use crate::settings::{default_config_dir, write_atomic, SettingsError};

/// 0-255 sRGB, the same space `properties::EditKind::Color` already edits in.
pub(crate) type Rgb = (u8, u8, u8);

/// The only class this feature tags — see `ROADMAP.md`'s own hedged wording
/// ("and perhaps any instance"), which this deliberately does not chase.
pub(crate) const FOLDER_CLASS: &str = "Folder";

/// `place file path -> folder Explorer path -> tag colour`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FolderColors(HashMap<String, HashMap<String, Rgb>>);

impl FolderColors {
    /// Reads the store; a missing file, an unreadable path, or a malformed
    /// document all fall back to an empty store rather than blocking
    /// startup — the same discipline `Settings::load` follows.
    pub(crate) fn load() -> FolderColors {
        let Some(path) = folder_colors_path() else {
            return FolderColors::default();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return FolderColors::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    /// Writes the same path `load` reads, via the same temp-file-then-rename
    /// discipline every other persisted file in this crate uses.
    pub(crate) fn save(&self) -> Result<(), SettingsError> {
        let path = folder_colors_path().ok_or(SettingsError::NoConfigDir)?;
        let bytes = serde_json::to_vec_pretty(self).expect("FolderColors always serializes");
        write_atomic(&path, &bytes)
    }

    pub(crate) fn get(&self, place: &Path, folder_path: &str) -> Option<Rgb> {
        self.0.get(&place_key(place))?.get(folder_path).copied()
    }

    pub(crate) fn set(&mut self, place: &Path, folder_path: &str, color: Rgb) {
        self.0
            .entry(place_key(place))
            .or_default()
            .insert(folder_path.to_owned(), color);
    }

    /// Drops every entry for `place` whose folder no longer exists (or is no
    /// longer a `Folder`) at its tagged path — see this module's doc comment.
    /// Returns whether anything changed, so a caller only re-saves when it
    /// did.
    pub(crate) fn prune(&mut self, place: &Path, dom: &WeakDom) -> bool {
        let Some(tagged) = self.0.get_mut(&place_key(place)) else {
            return false;
        };
        let valid = folder_paths(dom);
        let before = tagged.len();
        tagged.retain(|path, _| valid.contains(path));
        before != tagged.len()
    }
}

fn folder_colors_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("folder_colors.json"))
}

fn place_key(place: &Path) -> String {
    place.to_string_lossy().into_owned()
}

/// `reference`'s Explorer-style path, itself included
/// (`Workspace.Nested.MyFolder`) — the key every store entry above is kept
/// under. `None` when `reference` no longer resolves.
pub(crate) fn path_of(dom: &WeakDom, reference: Ref) -> Option<String> {
    let mut names = Vec::new();
    let mut current = Some(reference);
    while let Some(r) = current {
        names.push(dom.get(r)?.name().to_owned());
        current = dom.parent(r);
    }
    names.reverse();
    Some(names.join("."))
}

/// Every `Folder`'s current path, for [`FolderColors::prune`].
fn folder_paths(dom: &WeakDom) -> HashSet<String> {
    fn visit(dom: &WeakDom, refs: &[Ref], out: &mut HashSet<String>) {
        for &reference in refs {
            let Some(instance) = dom.get(reference) else {
                continue;
            };
            if instance.class() == FOLDER_CLASS {
                if let Some(path) = path_of(dom, reference) {
                    out.insert(path);
                }
            }
            visit(dom, instance.children(), out);
        }
    }
    let mut out = HashSet::new();
    visit(dom, dom.root_refs(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Instance;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    /// Guards every test that mutates `XDG_CONFIG_HOME` — see the identical
    /// guard in `settings.rs`'s tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_config_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rbx_studio_folder_colors_test_{}_{n}",
            std::process::id()
        ))
    }

    fn place() -> PathBuf {
        PathBuf::from("/tmp/example.rbxl")
    }

    fn dom_with_folder(path_segments: &[&str]) -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(1);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        let mut parent = workspace;
        let mut folder = workspace;
        for (i, name) in path_segments.iter().enumerate() {
            folder = Ref::new(2 + i as u32);
            dom.insert(Instance::new(folder, "Folder", *name));
            dom.set_parent(folder, Some(parent));
            parent = folder;
        }
        (dom, folder)
    }

    #[test]
    fn round_trips_through_disk() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let mut colors = FolderColors::default();
        colors.set(&place(), "Workspace.Nested", (10, 20, 30));
        colors.save().unwrap();

        let loaded = FolderColors::load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded.get(&place(), "Workspace.Nested"), Some((10, 20, 30)));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let loaded = FolderColors::load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded, FolderColors::default());
    }

    #[test]
    fn malformed_json_loads_as_empty_instead_of_crashing() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_config_dir();
        std::fs::create_dir_all(dir.join("rbx-native")).unwrap();
        std::fs::write(
            dir.join("rbx-native/folder_colors.json"),
            b"not json at all {{{",
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let loaded = FolderColors::load();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(loaded, FolderColors::default());
    }

    #[test]
    fn get_is_scoped_to_its_own_place() {
        let mut colors = FolderColors::default();
        colors.set(&place(), "Workspace.A", (1, 2, 3));

        assert_eq!(
            colors.get(&PathBuf::from("/tmp/other.rbxl"), "Workspace.A"),
            None
        );
        assert_eq!(colors.get(&place(), "Workspace.A"), Some((1, 2, 3)));
    }

    #[test]
    fn prune_drops_entries_whose_folder_no_longer_exists() {
        let (dom, _) = dom_with_folder(&["Kept"]);
        let mut colors = FolderColors::default();
        colors.set(&place(), "Workspace.Kept", (1, 1, 1));
        colors.set(&place(), "Workspace.Renamed", (2, 2, 2));

        let changed = colors.prune(&place(), &dom);

        assert!(changed);
        assert_eq!(colors.get(&place(), "Workspace.Kept"), Some((1, 1, 1)));
        assert_eq!(colors.get(&place(), "Workspace.Renamed"), None);
    }

    #[test]
    fn prune_leaves_a_fully_valid_place_unchanged() {
        let (dom, _) = dom_with_folder(&["Kept"]);
        let mut colors = FolderColors::default();
        colors.set(&place(), "Workspace.Kept", (1, 1, 1));

        let changed = colors.prune(&place(), &dom);

        assert!(!changed);
        assert_eq!(colors.get(&place(), "Workspace.Kept"), Some((1, 1, 1)));
    }

    #[test]
    fn path_of_includes_the_instance_itself_and_its_ancestors() {
        let (dom, folder) = dom_with_folder(&["Nested", "Inner"]);
        assert_eq!(
            path_of(&dom, folder).as_deref(),
            Some("Workspace.Nested.Inner")
        );
    }

    #[test]
    fn setting_a_colour_never_touches_the_dom() {
        let (dom, folder) = dom_with_folder(&["Tagged"]);
        let before = dom.get(folder).unwrap().properties().clone();

        let mut colors = FolderColors::default();
        colors.set(&place(), "Workspace.Tagged", (9, 9, 9));

        // `set` only ever mutates `FolderColors`'s own map — the `Instance`
        // it was computed from is a separate, untouched borrow.
        let after = dom.get(folder).unwrap().properties().clone();
        assert_eq!(before, after);
        assert!(
            after.is_empty(),
            "a Folder carries no properties by default"
        );
    }
}
