//! `GET /cloud/v2/universes/{id}`: display name, visibility, owner and root
//! place for one universe.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    /// Any value Roblox returns that isn't one of the above; kept verbatim
    /// rather than treated as an error so a new visibility tier doesn't break
    /// every caller of `universe()`.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    User(u64),
    Group(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Universe {
    pub display_name: String,
    pub visibility: Visibility,
    pub owner: Owner,
    pub root_place_id: u64,
}

#[derive(Deserialize)]
struct UniverseRaw {
    #[serde(rename = "displayName")]
    display_name: String,
    visibility: String,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(rename = "rootPlace")]
    root_place: String,
}

impl Client {
    pub fn universe(&self, universe_id: u64) -> Result<Universe, CloudError> {
        let url = format!("https://apis.roblox.com/cloud/v2/universes/{universe_id}");
        let response = self.get_raw(&url, true, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let raw: UniverseRaw = serde_json::from_slice(&response.body)?;
        parse_universe(raw)
    }
}

fn parse_universe(raw: UniverseRaw) -> Result<Universe, CloudError> {
    let owner = parse_owner(raw.user.as_deref(), raw.group.as_deref())?;
    let root_place_id = trailing_numeric_segment(&raw.root_place).ok_or_else(|| {
        CloudError::UnexpectedShape(format!("unparseable rootPlace path: {:?}", raw.root_place))
    })?;
    Ok(Universe {
        display_name: raw.display_name,
        visibility: parse_visibility(&raw.visibility),
        owner,
        root_place_id,
    })
}

fn parse_owner(user: Option<&str>, group: Option<&str>) -> Result<Owner, CloudError> {
    if let Some(user) = user.filter(|s| !s.is_empty()) {
        return trailing_numeric_segment(user)
            .map(Owner::User)
            .ok_or_else(|| {
                CloudError::UnexpectedShape(format!("unparseable user owner path: {user:?}"))
            });
    }
    if let Some(group) = group.filter(|s| !s.is_empty()) {
        return trailing_numeric_segment(group)
            .map(Owner::Group)
            .ok_or_else(|| {
                CloudError::UnexpectedShape(format!("unparseable group owner path: {group:?}"))
            });
    }
    Err(CloudError::UnexpectedShape(
        "universe response has neither a user nor a group owner".to_string(),
    ))
}

fn parse_visibility(raw: &str) -> Visibility {
    match raw {
        "PUBLIC" => Visibility::Public,
        "PRIVATE" => Visibility::Private,
        other => Visibility::Other(other.to_string()),
    }
}

/// `"universes/123/places/456"` -> `456`. Also handles the bare
/// `"users/925308243"` / `"groups/1"` owner path shape.
fn trailing_numeric_segment(path: &str) -> Option<u64> {
    path.rsplit('/').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"path":"universes/6053515322","displayName":"Pebble's Liminal Playground","description":"","user":"users/925308243","visibility":"PRIVATE","rootPlace":"universes/6053515322/places/17675488706","templateRootPlace":""}"#;

    #[test]
    fn parses_the_real_universe_payload() {
        let raw: UniverseRaw = serde_json::from_str(SAMPLE).unwrap();
        let universe = parse_universe(raw).unwrap();

        assert_eq!(universe.display_name, "Pebble's Liminal Playground");
        assert_eq!(universe.visibility, Visibility::Private);
        assert_eq!(universe.owner, Owner::User(925308243));
        assert_eq!(universe.root_place_id, 17675488706);
    }

    #[test]
    fn parses_a_group_owned_universe() {
        let raw = UniverseRaw {
            display_name: "Group Place".to_string(),
            visibility: "PUBLIC".to_string(),
            user: None,
            group: Some("groups/42".to_string()),
            root_place: "universes/1/places/2".to_string(),
        };
        let universe = parse_universe(raw).unwrap();
        assert_eq!(universe.owner, Owner::Group(42));
        assert_eq!(universe.visibility, Visibility::Public);
    }

    #[test]
    fn unknown_visibility_is_kept_verbatim_not_an_error() {
        assert_eq!(
            parse_visibility("SOMETHING_NEW"),
            Visibility::Other("SOMETHING_NEW".to_string())
        );
    }

    #[test]
    fn missing_owner_is_an_unexpected_shape_error() {
        let raw = UniverseRaw {
            display_name: "No Owner".to_string(),
            visibility: "PUBLIC".to_string(),
            user: None,
            group: None,
            root_place: "universes/1/places/2".to_string(),
        };
        assert!(matches!(
            parse_universe(raw),
            Err(CloudError::UnexpectedShape(_))
        ));
    }

    #[test]
    fn malformed_root_place_path_is_an_unexpected_shape_error() {
        let raw = UniverseRaw {
            display_name: "Bad Root".to_string(),
            visibility: "PUBLIC".to_string(),
            user: Some("users/1".to_string()),
            group: None,
            root_place: "not-a-path".to_string(),
        };
        assert!(matches!(
            parse_universe(raw),
            Err(CloudError::UnexpectedShape(_))
        ));
    }

    #[test]
    fn trailing_numeric_segment_extracts_the_last_path_component() {
        assert_eq!(
            trailing_numeric_segment("universes/6053515322/places/17675488706"),
            Some(17675488706)
        );
        assert_eq!(trailing_numeric_segment("users/925308243"), Some(925308243));
        assert_eq!(trailing_numeric_segment(""), None);
    }
}
