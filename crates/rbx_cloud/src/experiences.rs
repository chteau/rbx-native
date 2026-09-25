//! Combines the ways to discover experiences an API key can act on:
//! restricted scopes from [`Client::introspect`] (the only way to see
//! private universes), the public game listing of the key's owner, and —
//! when the key holds `legacy-group:manage` — the public listing of every
//! group the owner can manage. A private experience on an unrestricted key
//! stays invisible: Open Cloud has no "list my universes" endpoint.

use std::collections::HashMap;

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};
use crate::games::CreatorKind;
use crate::universes::{Owner, Visibility};

const MANAGEABLE_GROUPS_URL: &str =
    "https://apis.roblox.com/legacy-develop/v1/user/groups/canmanage";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experience {
    pub universe_id: u64,
    pub root_place_id: u64,
    pub name: String,
    pub visibility: Visibility,
    /// Personal or group-owned — what the Home screen groups by.
    pub owner: Owner,
}

/// A group the key's owner can manage, for naming group headings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Experiences {
    /// Sorted by universe id.
    pub experiences: Vec<Experience>,
    /// Empty unless the key holds `legacy-group:manage`.
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
        for universe_id in restricted_ids {
            // A single stale/deleted universe id in a scope aborts the whole
            // listing rather than silently hiding it; the caller sees exactly
            // which lookup failed via the propagated `CloudError`.
            let universe = self.universe(universe_id)?;
            by_universe.insert(
                universe_id,
                Experience {
                    universe_id,
                    root_place_id: universe.root_place_id,
                    name: universe.display_name,
                    visibility: universe.visibility,
                    owner: universe.owner,
                },
            );
        }

        let can_manage_groups = key_info
            .scopes
            .iter()
            .any(|s| s.name == "legacy-group" && s.operations.iter().any(|o| o == "manage"));
        let groups = if can_manage_groups {
            self.manageable_groups()?
        } else {
            Vec::new()
        };

        let mut public_games = self.public_games_of_user(key_info.authorized_user_id)?;
        for group in &groups {
            public_games.extend(self.public_games_of_group(group.id)?);
        }
        for game in public_games {
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

#[cfg(test)]
mod tests {
    use super::*;

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
