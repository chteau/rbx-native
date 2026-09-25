//! Finding an experience from a link to one of its places — Home's "Add by
//! place ID or URL", the way to reach a private experience that no listing
//! can show on an unrestricted key (Open Cloud has no "list my universes";
//! the Creator Dashboard's own `universes/v1/search` is cookie-only).

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};
use crate::experiences::Experience;

#[derive(Deserialize)]
struct UniverseOfPlaceRaw {
    #[serde(rename = "universeId")]
    universe_id: Option<u64>,
}

#[derive(Deserialize)]
struct UserRaw {
    #[serde(rename = "displayName")]
    display_name: String,
}

impl Client {
    /// `GET /universes/v1/places/{id}/universe`: anonymous, and answers for
    /// private places too (checked live, 2026-09-25). `None` for a place id
    /// that does not exist.
    pub fn universe_of_place(&self, place_id: u64) -> Result<Option<u64>, CloudError> {
        let url = format!("https://apis.roblox.com/universes/v1/places/{place_id}/universe");
        let response = self.get_raw(&url, false, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let raw: UniverseOfPlaceRaw = serde_json::from_slice(&response.body)?;
        Ok(raw.universe_id)
    }

    /// The experience `place_id` belongs to, with `root_place_id` set to
    /// `place_id` itself so opening it opens the place that was linked,
    /// not necessarily the start place. Reading the universe needs the key
    /// to reach it (a private one on someone else's account is a 403).
    pub fn experience_of_place(&self, place_id: u64) -> Result<Option<Experience>, CloudError> {
        let Some(universe_id) = self.universe_of_place(place_id)? else {
            return Ok(None);
        };
        let universe = self.universe(universe_id)?;
        Ok(Some(Experience {
            universe_id,
            root_place_id: place_id,
            name: universe.display_name,
            visibility: universe.visibility,
            owner: universe.owner,
        }))
    }

    /// A user's display name (`users.roblox.com/v1/users/{id}`, anonymous) —
    /// what the wizard shows for a key's `authorized_user_id`.
    pub fn user_display_name(&self, user_id: u64) -> Result<String, CloudError> {
        let url = format!("https://users.roblox.com/v1/users/{user_id}");
        let response = self.get_raw(&url, false, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let raw: UserRaw = serde_json::from_slice(&response.body)?;
        Ok(raw.display_name)
    }
}

/// The place id in what a user pastes: a bare id, a game page link
/// (`roblox.com/games/<id>/…`, with or without a locale segment), or a
/// Creator Dashboard place link (`…/experiences/<universe>/places/<id>/…`).
pub fn place_id_from_link(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Ok(id) = text.parse() {
        return Some(id);
    }
    let path = text.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').collect();
    segments
        .windows(2)
        .find(|pair| pair[0] == "games" || pair[0] == "places")
        .and_then(|pair| pair[1].parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_place_id_out_of_every_link_shape() {
        assert_eq!(place_id_from_link(" 17675488706 "), Some(17675488706));
        assert_eq!(
            place_id_from_link("https://www.roblox.com/games/17675488706/Pebbles-Liminal"),
            Some(17675488706)
        );
        assert_eq!(
            place_id_from_link(
                "https://www.roblox.com/fr/games/17675488706?privateServerLinkCode=x"
            ),
            Some(17675488706)
        );
        assert_eq!(
            place_id_from_link(
                "https://create.roblox.com/dashboard/creations/experiences/6053515322/places/17675488706/configure"
            ),
            Some(17675488706)
        );
        assert_eq!(
            place_id_from_link("https://www.roblox.com/users/1/profile"),
            None
        );
        assert_eq!(place_id_from_link("not a link"), None);
    }

    #[test]
    fn a_missing_place_has_no_universe() {
        let raw: UniverseOfPlaceRaw = serde_json::from_str(r#"{"universeId":null}"#).unwrap();
        assert_eq!(raw.universe_id, None);
    }
}
