//! The badges the People list shows by a name: the experience owner's and
//! Roblox's verified badge. The owner's mark goes to the user who owns the
//! experience (or owns its group) and, as the real client marks a group
//! game's developers too, to the player when their Open Cloud key can
//! manage the experience. Both come from the web API and are kept in the
//! avatar cache, `owners.json` (universe → owners) and `verified.json`
//! (user → badge), so an offline run still shows them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use rbx_cloud::{ApiKey, Client, Owner};

/// Who gets a badge in this run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Badges {
    pub(crate) owners: HashSet<i64>,
    pub(crate) verified: HashSet<i64>,
}

fn file(name: &str) -> Option<PathBuf> {
    rbx_assets::cache_root().map(|root| root.join("avatars").join(name))
}

fn load<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    file(name)
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn store<T: serde::Serialize>(name: &str, value: &T) {
    if let (Some(path), Ok(json)) = (file(name), serde_json::to_vec(value)) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, json);
    }
}

/// The badges for `player` (this editor's user; the test players are
/// noobs with no account) in the experience `universe` (0 when the place isn't published): asked of
/// Roblox, the cache standing in for what it can't answer.
pub(crate) fn resolve(universe: u64, player: i64) -> Badges {
    let client = Client::new(ApiKey::from_env_or_config());
    let mut owners: HashMap<u64, HashSet<i64>> = load("owners.json");
    if universe != 0 {
        let creator = client
            .experience_creator(universe)
            .ok()
            .and_then(|owner| match owner {
                Owner::User(id) => Some(id as i64),
                Owner::Group(group) => client.group_owner(group).ok()?.map(|id| id as i64),
            });
        if let Some(creator) = creator {
            let mut found: HashSet<i64> = [creator].into();
            if player > 0 && client.universe(universe).is_ok() {
                found.insert(player);
            }
            owners.insert(universe, found);
            store("owners.json", &owners);
        }
    }
    let mut known: HashMap<i64, bool> = load("verified.json");
    if player > 0 {
        if let Ok(info) = client.user_info(player as u64) {
            known.insert(player, info.verified);
        }
    }
    store("verified.json", &known);
    Badges {
        owners: owners.remove(&universe).unwrap_or_default(),
        verified: [player]
            .into_iter()
            .filter(|u| known.get(u) == Some(&true))
            .collect(),
    }
}
