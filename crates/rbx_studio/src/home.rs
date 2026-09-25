//! What the launcher needs underneath its windows: which window a launch
//! starts at, the Recent list, the New templates, and turning an experience
//! from the My Games list into a local place file the editor can open.
//!
//! The order is fixed: no key yet → the setup wizard → Home; a key already
//! stored → Home; a place picked or created on Home → the editor. A place
//! path on the command line skips both, as it always has.

use std::path::{Path, PathBuf};

use rbx_cloud::{Client, CloudError, Experience};
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};
use serde::{Deserialize, Serialize};

use crate::save::{self, Format};
use crate::settings::{default_config_dir, write_atomic, SettingsError};

/// Which window a launch opens first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Wizard,
    Home,
    Editor(PathBuf),
}

pub(crate) fn route(path: Option<PathBuf>, has_key: bool) -> Route {
    match path {
        Some(path) => Route::Editor(path),
        None if has_key => Route::Home,
        None => Route::Wizard,
    }
}

/// How many places Recent keeps.
const RECENT_LIMIT: usize = 20;

/// One Recent entry. The ids are the Roblox place a file was opened from,
/// so Save/Publish can target it without asking again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecentPlace {
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) universe_id: Option<u64>,
    #[serde(default)]
    pub(crate) place_id: Option<u64>,
    /// The experience's name, for Recent's linked pill.
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// When it was last opened, in Unix seconds; set by [`remember`].
    #[serde(default)]
    pub(crate) opened: Option<i64>,
}

/// Recent places, most recent first, dropping files that no longer exist.
pub(crate) fn recent() -> Vec<RecentPlace> {
    let Some(path) = recent_path() else {
        return Vec::new();
    };
    read_recent(&path)
}

/// Moves `place` to the top of Recent. A plain file open keeps whatever
/// Roblox link the same path was recorded with before.
pub(crate) fn remember(place: RecentPlace) -> Result<(), SettingsError> {
    let path = recent_path().ok_or(SettingsError::NoConfigDir)?;
    let list = with_remembered(read_recent(&path), place);
    let bytes = serde_json::to_vec_pretty(&list).expect("RecentPlace serializes");
    write_atomic(&path, &bytes)
}

fn recent_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("recent.json"))
}

fn read_recent(path: &Path) -> Vec<RecentPlace> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<RecentPlace>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|place| place.path.exists())
        .collect()
}

fn with_remembered(mut list: Vec<RecentPlace>, mut place: RecentPlace) -> Vec<RecentPlace> {
    place.opened = place
        .opened
        .or_else(|| Some(chrono::Utc::now().timestamp()));
    if let Some(index) = list.iter().position(|p| p.path == place.path) {
        let old = list.remove(index);
        if place.place_id.is_none() {
            place.universe_id = old.universe_id;
            place.place_id = old.place_id;
            place.name = place.name.or(old.name);
        }
    }
    list.insert(0, place);
    list.truncate(RECENT_LIMIT);
    list
}

/// Home's New tab. Flat Terrain is not here yet: nothing in the tree can
/// write `Terrain.SmoothGrid`, and a terrain template without its voxels
/// would be a Baseplate by another name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Template {
    Baseplate,
}

impl Template {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Template::Baseplate => "Baseplate",
        }
    }

    /// Writes a new binary place at `path`. Refuses to overwrite a file.
    pub(crate) fn create(self, path: &Path) -> Result<(), String> {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        save::save(&self.dom(), Format::Binary, path)
    }

    fn dom(self) -> WeakDom {
        let mut dom = WeakDom::new();
        // The services a fresh Studio place shows in the Explorer; a place
        // file's roots are its services, with no DataModel above them.
        let service = |dom: &mut WeakDom, class| dom.new_instance(class, class, None);
        let workspace = service(&mut dom, "Workspace");
        for class in [
            "Players",
            "Lighting",
            "ReplicatedFirst",
            "ReplicatedStorage",
            "ServerScriptService",
            "ServerStorage",
            "StarterGui",
            "StarterPack",
            "Teams",
            "SoundService",
            "TextChatService",
        ] {
            service(&mut dom, class);
        }
        let player = service(&mut dom, "StarterPlayer");
        dom.new_instance("StarterPlayerScripts", "StarterPlayerScripts", Some(player));
        dom.new_instance(
            "StarterCharacterScripts",
            "StarterCharacterScripts",
            Some(player),
        );
        dom.new_instance("Camera", "Camera", Some(workspace));
        dom.new_instance("Terrain", "Terrain", Some(workspace));

        // ponytail: sizes and placement as Studio's own Baseplate template
        // lays them out, from memory rather than a file saved by real
        // Studio; check against one before calling this parity.
        let set = |dom: &mut WeakDom, r, key: &str, value| {
            dom.set_property(r, key, value).expect("instance exists");
        };
        let at = |x, y, z| {
            Variant::CFrame(CFrameData {
                position: Vector3Data { x, y, z },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            })
        };
        let baseplate = dom.new_instance("Part", "Baseplate", Some(workspace));
        set(&mut dom, baseplate, "Anchored", Variant::Bool(true));
        set(&mut dom, baseplate, "Locked", Variant::Bool(true));
        set(
            &mut dom,
            baseplate,
            "size",
            Variant::Vector3(Vector3Data {
                x: 2048.0,
                y: 16.0,
                z: 2048.0,
            }),
        );
        set(&mut dom, baseplate, "CFrame", at(0.0, -8.0, 0.0));
        set(
            &mut dom,
            baseplate,
            "Color3uint8",
            Variant::Color3uint8 {
                r: 91,
                g: 91,
                b: 91,
            },
        );

        let spawn = dom.new_instance("SpawnLocation", "SpawnLocation", Some(workspace));
        set(&mut dom, spawn, "Anchored", Variant::Bool(true));
        set(
            &mut dom,
            spawn,
            "size",
            Variant::Vector3(Vector3Data {
                x: 12.0,
                y: 1.0,
                z: 12.0,
            }),
        );
        set(&mut dom, spawn, "CFrame", at(0.0, 0.5, 0.0));
        dom
    }
}

/// Where Home's New place writes: `places/Baseplate.rbxl`, or the first
/// free `Baseplate N.rbxl` beside it.
pub(crate) fn new_place_path(template: Template) -> Option<PathBuf> {
    let dir = places_dir()?;
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{}.rbxl", template.name())),
            n => dir.join(format!("{} {n}.rbxl", template.name())),
        })
        .find(|path| !path.exists())
}

/// Where the local copies of places opened from Roblox live: the config
/// directory, not the cache — this is the user's work until published.
fn places_dir() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("places"))
}

/// What [`open_experience`] found.
#[derive(Debug)]
pub(crate) enum Opened {
    /// A fresh download, ready to open.
    Downloaded(PathBuf),
    /// A local copy from an earlier open already exists and was left alone:
    /// it may hold unpublished work. Home asks whether to open it or pass
    /// `replace` to download the published version over it.
    LocalCopy(PathBuf),
}

/// The local copy an earlier open of `experience` left, if any.
pub(crate) fn local_copy(experience: &Experience) -> Option<PathBuf> {
    let dir = places_dir()?;
    let stem = format!("{}-{}", experience.universe_id, experience.root_place_id);
    ["rbxl", "rbxlx"]
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .into_iter()
        .find(|path| path.exists())
}

/// Why [`open_experience`] failed: the HTTP status when Roblox refused,
/// and a line for the user.
#[derive(Debug)]
pub(crate) struct OpenError {
    pub(crate) status: Option<u16>,
    pub(crate) message: String,
}

impl From<String> for OpenError {
    fn from(message: String) -> Self {
        OpenError {
            status: None,
            message,
        }
    }
}

/// Downloads `experience`'s root place into [`places_dir`] and records it in
/// Recent with its ids, so the editor can Save/Publish back to it.
/// Blocking: call it off the UI thread.
pub(crate) fn open_experience(
    client: &Client,
    experience: &Experience,
    replace: bool,
) -> Result<Opened, OpenError> {
    let dir = places_dir().ok_or("no config directory to download into".to_string())?;
    let stem = format!("{}-{}", experience.universe_id, experience.root_place_id);
    let existing = local_copy(experience);
    let path = match existing.clone() {
        Some(path) if !replace => Opened::LocalCopy(path),
        _ => {
            let bytes = client
                .download_place(experience.root_place_id)
                .map_err(|err| OpenError {
                    status: match &err {
                        CloudError::Http { status, .. } => Some(*status),
                        _ => None,
                    },
                    message: err.to_string(),
                })?;
            let ext = match Format::sniff(&bytes) {
                Format::Binary => "rbxl",
                Format::Xml => "rbxlx",
            };
            let path = dir.join(format!("{stem}.{ext}"));
            write_atomic(&path, &bytes).map_err(|err| err.to_string())?;
            // A replaced copy in the other format must not shadow this one.
            if let Some(old) = existing.filter(|old| *old != path) {
                let _ = std::fs::remove_file(old);
            }
            Opened::Downloaded(path)
        }
    };
    let file = match &path {
        Opened::Downloaded(path) | Opened::LocalCopy(path) => path.clone(),
    };
    remember(RecentPlace {
        path: file,
        universe_id: Some(experience.universe_id),
        place_id: Some(experience.root_place_id),
        name: Some(experience.name.clone()),
        opened: None,
    })
    .map_err(|err| err.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(path: &str, place_id: Option<u64>) -> RecentPlace {
        RecentPlace {
            path: PathBuf::from(path),
            universe_id: place_id.map(|id| id + 1000),
            place_id,
            name: None,
            opened: Some(1),
        }
    }

    #[test]
    fn a_launch_routes_wizard_then_home_then_editor() {
        assert_eq!(route(None, false), Route::Wizard);
        assert_eq!(route(None, true), Route::Home);
        let path = PathBuf::from("a.rbxl");
        assert_eq!(route(Some(path.clone()), false), Route::Editor(path));
    }

    #[test]
    fn remembering_moves_to_the_top_keeps_the_link_and_caps_the_list() {
        let list = vec![place("a", None), place("b", Some(7))];
        let list = with_remembered(list, place("b", None));
        assert_eq!(list, vec![place("b", Some(7)), place("a", None)]);

        let full: Vec<_> = (0..RECENT_LIMIT)
            .map(|i| place(&i.to_string(), None))
            .collect();
        let list = with_remembered(full, place("new", None));
        assert_eq!(list.len(), RECENT_LIMIT);
        assert_eq!(list[0].path, PathBuf::from("new"));
    }

    #[test]
    fn the_baseplate_template_writes_a_place_the_viewer_reads_back() {
        let path = std::env::temp_dir().join(format!("rbx_home_{}.rbxl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Template::Baseplate.create(&path).unwrap();
        assert!(
            Template::Baseplate.create(&path).is_err(),
            "never overwrites"
        );

        let dom = rbx_viewer::read_place(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let names: Vec<_> = dom
            .root_refs()
            .iter()
            .flat_map(|&r| dom.get(r).unwrap().children().to_vec())
            .map(|r| dom.get(r).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"Baseplate".to_string()), "{names:?}");
        assert!(names.contains(&"SpawnLocation".to_string()), "{names:?}");
    }
}
