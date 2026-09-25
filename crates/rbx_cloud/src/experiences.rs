//! Combines the ways to discover experiences an API key can act on:
//! restricted scopes from [`Client::introspect`] (the only way to see
//! private universes), the public game listing of the key's owner, and —
//! when the key holds `legacy-group:manage` — the groups the owner can
//! manage (their games one group at a time, [`Client::group_experiences`]),
//! and — with `user.inventory-item:read` — every
//! place the owner created, private ones included. Without that scope a
//! private experience on an unrestricted key stays invisible: Open Cloud has
//! no "list my universes" endpoint.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{self, CloudError};
use crate::games::CreatorKind;
use crate::universes::{Owner, Visibility};

const MANAGEABLE_GROUPS_URL: &str =
    "https://apis.roblox.com/legacy-develop/v1/user/groups/canmanage";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Experience {
    pub universe_id: u64,
    pub root_place_id: u64,
    pub name: String,
    pub visibility: Visibility,
    /// Personal or group-owned — what the Home screen groups by.
    pub owner: Owner,
}

/// A group the key's owner can manage, for naming group headings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Experiences {
    /// Sorted by universe id.
    pub experiences: Vec<Experience>,
    /// Empty unless the key holds `legacy-group:manage`. Their games are
    /// not in `experiences`: see [`Client::group_experiences`].
    pub groups: Vec<Group>,
}

#[derive(Deserialize)]
struct GroupsRaw {
    data: Vec<Group>,
}

impl Client {
    pub fn list_experiences(&self) -> Result<Experiences, CloudError> {
        let key_info = self.introspect()?;

        let mut restricted_ids: Vec<u64> = key_info
            .scopes
            .iter()
            .flat_map(|scope| scope.universe_ids.iter().copied())
            .collect();
        restricted_ids.sort_unstable();
        restricted_ids.dedup();

        let mut by_universe: HashMap<u64, Experience> = HashMap::new();
        // A single stale/deleted universe id in a scope aborts the whole
        // listing rather than silently hiding it; the caller sees exactly
        // which lookup failed via the propagated `CloudError`.
        for (universe_id, universe) in restricted_ids
            .iter()
            .copied()
            .zip(fan_out(&restricted_ids, |&id| self.universe(id)))
        {
            by_universe.insert(universe_id, experience(universe_id, universe?));
        }

        let has = |name: &str, operation: &str| {
            key_info
                .scopes
                .iter()
                .any(|s| s.name == name && s.operations.iter().any(|o| o == operation))
        };
        let groups = if has("legacy-group", "manage") {
            self.manageable_groups()?
        } else {
            Vec::new()
        };
        if has("user.inventory-item", "read") {
            self.add_created_places(key_info.authorized_user_id, &mut by_universe)?;
        }

        // A group's own games are not fetched here: Roblox rate-limits the
        // group listing per IP to a few requests every five seconds, so a
        // hundred groups would cost minutes. `group_experiences` fetches one
        // group when it is asked for.
        for game in self.public_games_of_user(key_info.authorized_user_id)? {
            let owner = match game.creator.kind {
                CreatorKind::User => Owner::User(game.creator.id),
                CreatorKind::Group => Owner::Group(game.creator.id),
            };
            by_universe.entry(game.universe_id).or_insert(Experience {
                universe_id: game.universe_id,
                root_place_id: game.root_place_id,
                name: game.name,
                visibility: Visibility::Public,
                owner,
            });
        }

        let mut experiences: Vec<Experience> = by_universe.into_values().collect();
        experiences.sort_unstable_by_key(|e| e.universe_id);
        Ok(Experiences {
            experiences,
            groups,
        })
    }

    /// Every universe behind a place the user created, private ones
    /// included (see `inventory`). Sub-places collapse onto their universe;
    /// one lookup per new universe.
    fn add_created_places(
        &self,
        user_id: u64,
        by_universe: &mut HashMap<u64, Experience>,
    ) -> Result<(), CloudError> {
        let places = self.created_places(user_id)?;
        // A place whose universe is gone (deleted, archived, taken down) is
        // skipped rather than failing the listing: this list is the user's
        // whole history of created places, old ones included.
        let mut universes: Vec<u64> = fan_out(&places, |&place| self.universe_of_place(place))
            .into_iter()
            .filter_map(|found| found.ok().flatten())
            .filter(|id| !by_universe.contains_key(id))
            .collect();
        universes.sort_unstable();
        universes.dedup();
        for (universe_id, universe) in universes
            .iter()
            .copied()
            .zip(fan_out(&universes, |&id| self.universe(id)))
        {
            if let Ok(universe) = universe {
                by_universe.insert(universe_id, experience(universe_id, universe));
            }
        }
        Ok(())
    }

    /// One group's public experiences. Private group experiences only show
    /// through a key restricted to them (see [`Client::list_experiences`]).
    pub fn group_experiences(&self, group_id: u64) -> Result<Vec<Experience>, CloudError> {
        Ok(self
            .public_games_of_group(group_id)?
            .into_iter()
            .map(|game| Experience {
                universe_id: game.universe_id,
                root_place_id: game.root_place_id,
                name: game.name,
                visibility: Visibility::Public,
                owner: Owner::Group(group_id),
            })
            .collect())
    }

    /// `GET /legacy-develop/v1/user/groups/canmanage` (`legacy-group:manage`).
    pub fn manageable_groups(&self) -> Result<Vec<Group>, CloudError> {
        let response = self.get_raw(MANAGEABLE_GROUPS_URL, true, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                MANAGEABLE_GROUPS_URL,
                response.status,
                &response.headers,
            ));
        }
        let raw: GroupsRaw = serde_json::from_slice(&response.body)?;
        Ok(raw.data)
    }
}

fn experience(universe_id: u64, universe: crate::universes::Universe) -> Experience {
    Experience {
        universe_id,
        root_place_id: universe.root_place_id,
        name: universe.display_name,
        visibility: universe.visibility,
        owner: universe.owner,
    }
}

/// How many requests [`fan_out`] keeps in flight: enough to turn a
/// hundred sequential round trips into a few seconds, few enough to stay
/// clear of Roblox's per-IP limits (a 429 is retried by `retry` anyway).
const PARALLEL: usize = 8;

/// `f` over every item on up to [`PARALLEL`] threads, results in the
/// items' order. The client is blocking, so this is plain scoped threads.
fn fan_out<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..PARALLEL.min(items.len()) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else {
                    break;
                };
                let result = f(item);
                results.lock().unwrap_or_else(|e| e.into_inner())[index] = Some(result);
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .map(|result| result.expect("every index was taken exactly once"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_out_keeps_the_items_order() {
        let items: Vec<u32> = (0..50).collect();
        let doubled = fan_out(&items, |&n| {
            std::thread::sleep(std::time::Duration::from_micros(u64::from(50 - n)));
            n * 2
        });
        assert_eq!(doubled, (0..50).map(|n| n * 2).collect::<Vec<_>>());
        assert!(fan_out(&[] as &[u32], |&n| n).is_empty());
    }

    #[test]
    fn parses_the_documented_groups_payload() {
        let raw: GroupsRaw =
            serde_json::from_str(r#"{"data":[{"id":7,"name":"Studio Team"}]}"#).unwrap();
        assert_eq!(
            raw.data,
            vec![Group {
                id: 7,
                name: "Studio Team".into()
            }]
        );
    }
}
