//! Combines the two ways to discover experiences an API key can act on:
//! restricted scopes from [`Client::introspect`] (the only way to see
//! private universes) and the public game listing for the same user.

use std::collections::HashMap;

use crate::client::Client;
use crate::error::CloudError;
use crate::universes::Visibility;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experience {
    pub universe_id: u64,
    pub root_place_id: u64,
    pub name: String,
    pub visibility: Visibility,
}

impl Client {
    pub fn list_experiences(&self) -> Result<Vec<Experience>, CloudError> {
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
                },
            );
        }

        let public_games = self.public_games_of_user(key_info.authorized_user_id)?;
        for game in public_games {
            by_universe.entry(game.universe_id).or_insert(Experience {
                universe_id: game.universe_id,
                root_place_id: game.root_place_id,
                name: game.name,
                visibility: Visibility::Public,
            });
        }

        let mut experiences: Vec<Experience> = by_universe.into_values().collect();
        experiences.sort_unstable_by_key(|e| e.universe_id);
        Ok(experiences)
    }
}
