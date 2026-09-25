//! My Games on disk, so Home shows the last listing at once and refreshes
//! it in the background instead of waiting on Roblox every launch: the
//! personal listing, each group's games as they were last opened, and the
//! experiences' icons with the URL each was downloaded from.
//!
//! One file per Roblox account (`launcher/games-<user id>.json` under the
//! cache root), so switching keys between accounts never shows the other
//! account's games. Disposable: anything unreadable is treated as empty.

use std::collections::HashMap;
use std::path::PathBuf;

use rbx_cloud::{Experience, Experiences};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct GamesCache {
    pub(super) listing: Experiences,
    /// Group id → that group's games, as last fetched.
    #[serde(default)]
    pub(super) group_games: HashMap<u64, Vec<Experience>>,
    /// Universe id → the icon URL its cached PNG came from; a changed URL
    /// means a new icon to download.
    #[serde(default)]
    pub(super) icon_urls: HashMap<u64, String>,
}

fn dir() -> Option<PathBuf> {
    rbx_assets::cache_root().map(|root| root.join("launcher"))
}

fn file(user_id: u64) -> Option<PathBuf> {
    dir().map(|dir| dir.join(format!("games-{user_id}.json")))
}

fn icon_file(universe_id: u64) -> Option<PathBuf> {
    dir().map(|dir| dir.join("icons").join(format!("{universe_id}.png")))
}

impl GamesCache {
    pub(super) fn load(user_id: u64) -> Option<GamesCache> {
        let bytes = std::fs::read(file(user_id)?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Best effort: a cache that can't be written only costs the next
    /// launch a wait. Also records `user_id` as the account the next launch
    /// shows before the key has been checked.
    pub(super) fn save(&self, user_id: u64) {
        let Some(path) = file(user_id) else {
            return;
        };
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = write(&path, &bytes);
        }
        if let Some(dir) = dir() {
            let _ = write(&dir.join("last-user"), user_id.to_string().as_bytes());
        }
    }

    /// The account whose listing was saved last: shown at once on the next
    /// launch, then confirmed (or dropped) once the key is checked.
    pub(super) fn last_user() -> Option<u64> {
        std::fs::read_to_string(dir()?.join("last-user"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

pub(super) fn read_icon(universe_id: u64) -> Option<Vec<u8>> {
    std::fs::read(icon_file(universe_id)?).ok()
}

/// Icons are kept at this size: a card draws one at about 180 px, and a
/// 512 px PNG takes several times as long to decode on the next launch.
const ICON_SIZE: u32 = 256;

/// Stores `png` shrunk to [`ICON_SIZE`]; the original if it can't be read.
pub(super) fn write_icon(universe_id: u64, png: &[u8]) {
    let Some(path) = icon_file(universe_id) else {
        return;
    };
    let small = image::load_from_memory(png).ok().and_then(|icon| {
        let mut out = std::io::Cursor::new(Vec::new());
        icon.thumbnail(ICON_SIZE, ICON_SIZE)
            .write_to(&mut out, image::ImageFormat::Png)
            .ok()?;
        Some(out.into_inner())
    });
    let _ = write(&path, small.as_deref().unwrap_or(png));
}

/// Temp file then rename, so a reader never sees half a file.
fn write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_cloud::{Group, Owner, Visibility};

    #[test]
    fn a_cache_round_trips_through_json() {
        let game = Experience {
            universe_id: 1,
            root_place_id: 2,
            name: "A".into(),
            visibility: Visibility::Other("FRIENDS".into()),
            owner: Owner::Group(7),
        };
        let cache = GamesCache {
            listing: Experiences {
                experiences: vec![game.clone()],
                groups: vec![Group {
                    id: 7,
                    name: "G".into(),
                }],
            },
            group_games: HashMap::from([(7, vec![game])]),
            icon_urls: HashMap::from([(1, "https://t7.rbxcdn.com/x".into())]),
        };
        let json = serde_json::to_vec(&cache).unwrap();
        let back: GamesCache = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.listing, cache.listing);
        assert_eq!(back.group_games, cache.group_games);
        assert_eq!(back.icon_urls, cache.icon_urls);
    }
}
