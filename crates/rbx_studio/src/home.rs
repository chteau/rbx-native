//! What the launcher needs underneath its windows: which window a launch
//! starts at, the Recent list, the New templates, and turning an experience
//! from the My Games list into a local place file the editor can open.
//!
//! The order is fixed: a launch that needs setup (no or expired key or
//! sign-in, or `--setup`) → the setup wizard → Home; otherwise Home; a
//! place picked or created on Home → the editor. A place path on the
//! command line skips both, as it always has. "Skip for now" in the wizard
//! is remembered ([`skip_key`]): a launch with no key then goes straight to
//! Home, which offers Manage key; `--setup` still forces the wizard.

use std::path::{Path, PathBuf};

use rbx_cloud::{Client, CloudError, Experience};
use serde::{Deserialize, Serialize};

use crate::launcher::setup::SetupPlan;
use crate::save::Format;
use crate::settings::{default_config_dir, write_atomic, SettingsError};

/// Which window a launch opens first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Wizard(SetupPlan),
    Home,
    Editor(PathBuf),
}

pub(crate) fn route(path: Option<PathBuf>, setup: Option<SetupPlan>) -> Route {
    match (path, setup) {
        (Some(path), _) => Route::Editor(path),
        (None, Some(plan)) => Route::Wizard(plan),
        (None, None) => Route::Home,
    }
}

/// Whether the user skipped the key steps of the setup wizard.
pub(crate) fn key_skipped() -> bool {
    key_skipped_path().is_some_and(|path| path.exists())
}

/// Remembers that the key steps were skipped, so the wizard stops coming back.
pub(crate) fn skip_key() -> Result<(), SettingsError> {
    let path = key_skipped_path().ok_or(SettingsError::NoConfigDir)?;
    write_atomic(&path, b"")
}

fn key_skipped_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("key_skipped"))
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
/// Roblox link the same path was recorded with before, even one that fell
/// off Recent long ago: links are also kept in [`Links`], which forgets
/// nothing.
pub(crate) fn remember(place: RecentPlace) -> Result<(), SettingsError> {
    let path = recent_path().ok_or(SettingsError::NoConfigDir)?;
    let links_file = links_path().ok_or(SettingsError::NoConfigDir)?;
    let list = read_recent(&path);
    let mut links = read_links(&links_file);
    let before = links.clone();
    // Recent was the only home of links before links.json existed.
    for old in &list {
        if let Some(link) = Link::of(old) {
            links.entry(link_key(&old.path)).or_insert(link);
        }
    }
    let place = with_link(&mut links, place);
    if links != before {
        let bytes = serde_json::to_vec_pretty(&links).expect("Links serialize");
        write_atomic(&links_file, &bytes)?;
    }
    let list = with_remembered(list, place);
    let bytes = serde_json::to_vec_pretty(&list).expect("RecentPlace serializes");
    write_atomic(&path, &bytes)
}

/// Drops `path` from Recent. Its link stays in `links.json`, so the file
/// still knows its place when it is opened again.
pub(crate) fn forget(path: &Path) -> Result<(), SettingsError> {
    let file = recent_path().ok_or(SettingsError::NoConfigDir)?;
    let list = without(read_recent(&file), path);
    let bytes = serde_json::to_vec_pretty(&list).expect("RecentPlace serializes");
    write_atomic(&file, &bytes)
}

fn without(mut list: Vec<RecentPlace>, path: &Path) -> Vec<RecentPlace> {
    list.retain(|place| place.path != path);
    list
}

fn recent_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("recent.json"))
}

/// A file's Roblox place: what Save/Publish to Roblox targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Link {
    pub(crate) universe_id: u64,
    pub(crate) place_id: u64,
    /// The experience's name, for Recent's linked pill.
    #[serde(default)]
    pub(crate) name: Option<String>,
}

impl Link {
    fn of(place: &RecentPlace) -> Option<Link> {
        Some(Link {
            universe_id: place.universe_id?,
            place_id: place.place_id?,
            name: place.name.clone(),
        })
    }
}

/// Every file's link, by canonical path, in `links.json`. Unlike Recent it
/// has no cap; an entry goes only when its file is relinked.
type Links = std::collections::BTreeMap<PathBuf, Link>;

fn links_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("links.json"))
}

fn read_links(path: &Path) -> Links {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Paths are compared canonicalized: the editor records its file that way,
/// Home records the download path as it built it.
fn link_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or(path.to_path_buf())
}

/// The Roblox place `path` is linked to, if any.
pub(crate) fn link_of(path: &Path) -> Option<Link> {
    read_links(&links_path()?).remove(&link_key(path))
}

/// Records `place`'s link in `links` when it carries one, or fills its ids
/// in from `links` when it doesn't. A link to the same place keeps the name
/// it had when the new one comes without.
fn with_link(links: &mut Links, mut place: RecentPlace) -> RecentPlace {
    let key = link_key(&place.path);
    match Link::of(&place) {
        Some(mut link) => {
            if let Some(old) = links.get(&key).filter(|old| old.place_id == link.place_id) {
                link.name = link.name.or(old.name.clone());
            }
            place.name = link.name.clone();
            links.insert(key, link);
        }
        None => {
            if let Some(link) = links.get(&key) {
                place.universe_id = Some(link.universe_id);
                place.place_id = Some(link.place_id);
                place.name = link.name.clone();
            }
        }
    }
    place
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

mod game;
mod template;
pub(crate) use game::open_whole_game;
pub(crate) use template::Template;

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
/// directory, not the cache, this is the user's work until published.
pub(crate) fn places_dir() -> Option<PathBuf> {
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

impl OpenError {
    fn from_cloud(err: CloudError) -> Self {
        OpenError {
            status: match &err {
                CloudError::Http { status, .. } => Some(*status),
                _ => None,
            },
            message: err.to_string(),
        }
    }
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
                .map_err(OpenError::from_cloud)?;
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
        let plan = SetupPlan::KEY_ONLY;
        assert_eq!(route(None, Some(plan)), Route::Wizard(plan));
        assert_eq!(route(None, None), Route::Home);
        let path = PathBuf::from("a.rbxl");
        assert_eq!(route(Some(path.clone()), Some(plan)), Route::Editor(path));
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
    fn forgetting_drops_only_that_place() {
        let list = vec![place("a", None), place("b", Some(7))];
        assert_eq!(without(list, Path::new("a")), vec![place("b", Some(7))]);
    }

    #[test]
    fn a_link_outlives_its_file_falling_off_recent() {
        let mut links = Links::new();
        let mut linked = place("linked", Some(7));
        linked.name = Some("Pebbles".to_string());
        let mut list = with_remembered(Vec::new(), with_link(&mut links, linked));
        for i in 0..RECENT_LIMIT {
            list = with_remembered(list, with_link(&mut links, place(&i.to_string(), None)));
        }
        assert!(list.iter().all(|p| p.path != Path::new("linked")));

        // Opened again as a plain file: the link and its name come back.
        let back = with_link(&mut links, place("linked", None));
        assert_eq!(
            (back.universe_id, back.place_id, back.name.as_deref()),
            (Some(1007), Some(7), Some("Pebbles"))
        );
    }

    #[test]
    fn relinking_keeps_the_name_only_for_the_same_place() {
        let mut links = Links::new();
        let mut first = place("a", Some(7));
        first.name = Some("Pebbles".to_string());
        with_link(&mut links, first);

        // Same place, no name this time: the name stays.
        let same = with_link(&mut links, place("a", Some(7)));
        assert_eq!(same.name.as_deref(), Some("Pebbles"));

        // Another place without a name: the old one would be wrong.
        let other = with_link(&mut links, place("a", Some(8)));
        assert_eq!(other.name, None);
        assert_eq!(links[&PathBuf::from("a")].place_id, 8);

        let mut named = place("a", Some(9));
        named.name = Some("Liminal".to_string());
        with_link(&mut links, named);
        assert_eq!(links[&PathBuf::from("a")].name.as_deref(), Some("Liminal"));
    }
}
