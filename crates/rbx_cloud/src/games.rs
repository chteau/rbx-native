//! `GET games.roblox.com/v2/{users,groups}/{id}/games...`: public game
//! listings, unauthenticated. Private universes never appear here.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

const USER_GAMES_URL: &str = "https://games.roblox.com/v2/users";
const GROUP_GAMES_URL: &str = "https://games.roblox.com/v2/groups";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorKind {
    User,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Creator {
    pub id: u64,
    pub kind: CreatorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSummary {
    pub universe_id: u64,
    pub root_place_id: u64,
    pub name: String,
    pub creator: Creator,
}

#[derive(Deserialize)]
struct GamesPage {
    #[serde(rename = "nextPageCursor")]
    next_page_cursor: Option<String>,
    data: Vec<GameRaw>,
}

#[derive(Deserialize)]
struct GameRaw {
    id: u64,
    name: String,
    #[serde(rename = "rootPlace")]
    root_place: RootPlaceRaw,
    creator: CreatorRaw,
}

#[derive(Deserialize)]
struct RootPlaceRaw {
    id: u64,
}

#[derive(Deserialize)]
struct CreatorRaw {
    id: u64,
    #[serde(rename = "type")]
    kind: String,
}

impl Client {
    pub fn public_games_of_user(&self, user_id: u64) -> Result<Vec<GameSummary>, CloudError> {
        self.paged_public_games(&format!("{USER_GAMES_URL}/{user_id}/games"))
    }

    pub fn public_games_of_group(&self, group_id: u64) -> Result<Vec<GameSummary>, CloudError> {
        self.paged_public_games(&format!("{GROUP_GAMES_URL}/{group_id}/gamesV2"))
    }

    fn paged_public_games(&self, base_url: &str) -> Result<Vec<GameSummary>, CloudError> {
        let mut games = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let response = self.get_with(base_url, false, true, |req| {
                let req = req.query("accessFilter", "Public").query("limit", "50");
                match &cursor {
                    Some(c) => req.query("cursor", c),
                    None => req,
                }
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    base_url,
                    response.status,
                    &response.headers,
                ));
            }

            let page: GamesPage = serde_json::from_slice(&response.body)?;
            games.extend(page.data.into_iter().map(GameSummary::from));

            match page.next_page_cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => break,
            }
        }

        Ok(games)
    }
}

impl From<GameRaw> for GameSummary {
    fn from(raw: GameRaw) -> Self {
        GameSummary {
            universe_id: raw.id,
            root_place_id: raw.root_place.id,
            name: raw.name,
            creator: Creator {
                id: raw.creator.id,
                kind: CreatorKind::from(raw.creator.kind.as_str()),
            },
        }
    }
}

impl From<&str> for CreatorKind {
    fn from(value: &str) -> Self {
        match value {
            "Group" => CreatorKind::Group,
            // Fail open: an unrecognized creator type shouldn't break the
            // whole listing, and "User" is by far the common case.
            _ => CreatorKind::User,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PAGE: &str = r#"{"previousPageCursor":null,"nextPageCursor":"abc","data":[{"id":9828239630,"name":"Fragment - Demo","rootPlace":{"id":103912637612783},"creator":{"id":925308243,"type":"User"},"placeVisits":36}]}"#;

    #[test]
    fn parses_the_real_games_page_payload() {
        let page: GamesPage = serde_json::from_str(SAMPLE_PAGE).unwrap();
        assert_eq!(page.next_page_cursor.as_deref(), Some("abc"));
        assert_eq!(page.data.len(), 1);

        let summary: GameSummary = page.data.into_iter().next().unwrap().into();
        assert_eq!(summary.universe_id, 9828239630);
        assert_eq!(summary.root_place_id, 103912637612783);
        assert_eq!(summary.name, "Fragment - Demo");
        assert_eq!(
            summary.creator,
            Creator {
                id: 925308243,
                kind: CreatorKind::User
            }
        );
    }

    #[test]
    fn last_page_has_no_next_cursor() {
        let json = r#"{"previousPageCursor":"x","nextPageCursor":null,"data":[]}"#;
        let page: GamesPage = serde_json::from_str(json).unwrap();
        assert!(page.next_page_cursor.is_none());
        assert!(page.data.is_empty());
    }

    #[test]
    fn group_creator_type_is_recognized() {
        assert_eq!(CreatorKind::from("Group"), CreatorKind::Group);
    }

    #[test]
    fn unknown_creator_type_falls_back_to_user() {
        assert_eq!(CreatorKind::from("Robot"), CreatorKind::User);
    }
}
