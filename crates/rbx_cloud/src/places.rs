//! Finding an experience from a link to one of its places, Home's "Add by
//! place ID or URL", the way to reach a private experience that no listing
//! can show on an unrestricted key (Open Cloud has no "list my universes";
//! the Creator Dashboard's own `universes/v1/search` is cookie-only).

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{self, CloudError};
use crate::experiences::Experience;
use crate::universes::Owner;

#[derive(Deserialize)]
struct UniverseOfPlaceRaw {
    #[serde(rename = "universeId")]
    universe_id: Option<u64>,
}

/// One place of an experience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceSummary {
    pub id: u64,
    pub name: String,
}

#[derive(Deserialize)]
struct PlacesPageRaw {
    #[serde(rename = "nextPageCursor")]
    next_page_cursor: Option<String>,
    data: Vec<PlaceSummary>,
}

/// One page of `develop.roblox.com`'s place list: its places and the cursor
/// of the next page, `None` on the last.
fn parse_places_page(
    body: &[u8],
) -> Result<(Vec<PlaceSummary>, Option<String>), serde_json::Error> {
    let raw: PlacesPageRaw = serde_json::from_slice(body)?;
    Ok((raw.data, raw.next_page_cursor))
}

#[derive(Deserialize)]
struct UserRaw {
    #[serde(default)]
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(default, rename = "hasVerifiedBadge")]
    has_verified_badge: bool,
}

/// A user as `users.roblox.com/v1/users/{id}` describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
    pub name: String,
    pub display_name: String,
    /// The account carries Roblox's verified badge.
    pub verified: bool,
}

#[derive(Deserialize)]
struct GamesRaw {
    data: Vec<GameRaw>,
}

#[derive(Deserialize)]
struct GameRaw {
    creator: CreatorRaw,
}

#[derive(Deserialize)]
struct CreatorRaw {
    id: u64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct GroupRaw {
    owner: Option<GroupOwnerRaw>,
}

#[derive(Deserialize)]
struct GroupOwnerRaw {
    #[serde(rename = "userId")]
    user_id: u64,
}

/// `POST users.roblox.com/v1/users` takes at most this many ids per call
/// (`MultiGetByUserIdRequest.userIds.maxItems` in `openapi.json`).
const USERS_PER_CALL: usize = 200;

#[derive(Serialize)]
struct UsersRequest<'a> {
    #[serde(rename = "userIds")]
    user_ids: &'a [u64],
    #[serde(rename = "excludeBannedUsers")]
    exclude_banned_users: bool,
}

#[derive(Deserialize)]
struct UsersRaw {
    data: Vec<UserIdRaw>,
}

#[derive(Deserialize)]
struct UserIdRaw {
    id: u64,
    #[serde(rename = "displayName")]
    display_name: String,
}

impl Client {
    /// `GET /universes/v1/places/{id}/universe`: anonymous, and answers for
    /// private places too (checked live, 2026-09-25). `None` for a place id
    /// that does not exist.
    pub fn universe_of_place(&self, place_id: u64) -> Result<Option<u64>, CloudError> {
        let url = self.apis_url(&format!("/universes/v1/places/{place_id}/universe"));
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

    /// Every place of an experience, start place included, oldest first:
    /// `GET develop.roblox.com/v1/universes/{id}/places`, page by page.
    /// Anonymous; it answered for a private test universe too (checked
    /// live, 2026-10-20). Open Cloud itself has no place listing.
    pub fn universe_places(&self, universe_id: u64) -> Result<Vec<PlaceSummary>, CloudError> {
        let mut places = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut url = format!(
                "https://develop.roblox.com/v1/universes/{universe_id}/places?sortOrder=Asc&limit=100"
            );
            if let Some(cursor) = &cursor {
                url.push_str(&format!("&cursor={cursor}"));
            }
            let response = self.get_raw(&url, false, true)?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    &url,
                    response.status,
                    &response.headers,
                ));
            }
            let (page, next) = parse_places_page(&response.body)?;
            places.extend(page);
            match next {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(places),
            }
        }
    }

    /// A user's display name (`users.roblox.com/v1/users/{id}`, anonymous),
    /// what the wizard shows for a key's `authorized_user_id`.
    pub fn user_display_name(&self, user_id: u64) -> Result<String, CloudError> {
        self.user_names(user_id)
            .map(|(_, display_name)| display_name)
    }

    /// A user's `(username, display name)`, from the same anonymous lookup
    /// as [`Client::user_display_name`].
    pub fn user_names(&self, user_id: u64) -> Result<(String, String), CloudError> {
        self.user_info(user_id)
            .map(|info| (info.name, info.display_name))
    }

    /// A user's names and verified badge, from the same anonymous lookup
    /// as [`Client::user_display_name`].
    pub fn user_info(&self, user_id: u64) -> Result<UserInfo, CloudError> {
        let url = format!("https://users.roblox.com/v1/users/{user_id}");
        let raw: UserRaw = self.anonymous_json(&url)?;
        Ok(UserInfo {
            name: raw.name,
            display_name: raw.display_name,
            verified: raw.has_verified_badge,
        })
    }

    /// Who made an experience (`games.roblox.com/v1/games`, anonymous, so
    /// it answers for any public experience, not only the key's).
    pub fn experience_creator(&self, universe_id: u64) -> Result<Owner, CloudError> {
        let url = format!("https://games.roblox.com/v1/games?universeIds={universe_id}");
        let raw: GamesRaw = self.anonymous_json(&url)?;
        let creator = raw
            .data
            .into_iter()
            .next()
            .ok_or_else(|| CloudError::UnexpectedShape(format!("no experience {universe_id}")))?
            .creator;
        Ok(if creator.kind == "Group" {
            Owner::Group(creator.id)
        } else {
            Owner::User(creator.id)
        })
    }

    /// The user who owns a group (`groups.roblox.com/v1/groups/{id}`,
    /// anonymous); `None` for an ownerless group.
    pub fn group_owner(&self, group_id: u64) -> Result<Option<u64>, CloudError> {
        let url = format!("https://groups.roblox.com/v1/groups/{group_id}");
        let raw: GroupRaw = self.anonymous_json(&url)?;
        Ok(raw.owner.map(|o| o.user_id))
    }

    fn anonymous_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, CloudError> {
        let response = self.get_raw(url, false, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                url,
                response.status,
                &response.headers,
            ));
        }
        Ok(serde_json::from_slice(&response.body)?)
    }

    /// Display names for many users at once (anonymous). A deleted account
    /// is simply missing from the answer; banned ones are kept, since a
    /// place's history still names them.
    pub fn user_display_names(&self, user_ids: &[u64]) -> Result<Vec<(u64, String)>, CloudError> {
        let url = "https://users.roblox.com/v1/users";
        let mut names = Vec::with_capacity(user_ids.len());
        for chunk in user_ids.chunks(USERS_PER_CALL) {
            let body = UsersRequest {
                user_ids: chunk,
                exclude_banned_users: false,
            };
            let response = self.post_json_raw(url, false, &body)?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    url,
                    response.status,
                    &response.headers,
                ));
            }
            let raw: UsersRaw = serde_json::from_slice(&response.body)?;
            names.extend(raw.data.into_iter().map(|u| (u.id, u.display_name)));
        }
        Ok(names)
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
    fn reads_a_page_of_places_and_its_cursor() {
        let body = br#"{"previousPageCursor":null,"nextPageCursor":"abc_2","data":[
            {"id":1,"universeId":9,"name":"Lobby","description":""},
            {"id":2,"universeId":9,"name":"Arena","description":"x"}]}"#;
        let (places, next) = parse_places_page(body).unwrap();
        assert_eq!(
            places,
            vec![
                PlaceSummary {
                    id: 1,
                    name: "Lobby".into()
                },
                PlaceSummary {
                    id: 2,
                    name: "Arena".into()
                },
            ]
        );
        assert_eq!(next.as_deref(), Some("abc_2"));
        let last = br#"{"previousPageCursor":"abc_1","nextPageCursor":null,"data":[]}"#;
        assert_eq!(parse_places_page(last).unwrap(), (Vec::new(), None));
    }

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
    fn the_users_request_and_answer_have_the_real_shape() {
        let body = serde_json::to_string(&UsersRequest {
            user_ids: &[925308243, 1],
            exclude_banned_users: false,
        })
        .unwrap();
        assert_eq!(
            body,
            r#"{"userIds":[925308243,1],"excludeBannedUsers":false}"#
        );
        // users.roblox.com, 2026-10-04.
        let raw: UsersRaw = serde_json::from_str(r#"{"data":[{"hasVerifiedBadge":true,"id":1,"name":"Roblox","displayName":"Roblox"},{"hasVerifiedBadge":true,"id":925308243,"name":"Cheeteau","displayName":"Cheeteau"}]}"#).unwrap();
        let names: Vec<(u64, String)> = raw
            .data
            .into_iter()
            .map(|u| (u.id, u.display_name))
            .collect();
        assert_eq!(
            names,
            [
                (1, "Roblox".to_string()),
                (925308243, "Cheeteau".to_string())
            ]
        );
    }

    #[test]
    fn a_missing_place_has_no_universe() {
        let raw: UniverseOfPlaceRaw = serde_json::from_str(r#"{"universeId":null}"#).unwrap();
        assert_eq!(raw.universe_id, None);
    }
}
