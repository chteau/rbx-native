//! Who can reach an experience: whether it is public, and Security's
//! "Enable Studio Access to API Services". Open Cloud only reads the first
//! (`Universe.visibility` is read-only) and has no field for the second, so
//! both run as the web sign-in (see `web`), on the routes Studio's Game
//! Settings and the Creator Hub use.
//!
//! Public or private is `develop` `POST /v1/universes/{id}/activate` or
//! `/deactivate` (no body), read back as `isActive` on `GET /v1/universes/{id}`.
//! The API switch is `studioAccessToApisAllowed` on
//! `/v2/universes/{id}/configuration` (GET, then a PATCH of that field
//! alone), the route Studio's Security page and the Creator Hub use; the v1
//! configuration doesn't carry it.

use serde::Deserialize;

use crate::error::CloudError;
use crate::heartbeats::SessionCookie;
use crate::web::{self, Method, DEVELOP};

const API_FIELD: &str = "studioAccessToApisAllowed";
const APIS: &str = "https://apis.roblox.com";

/// Whether the experience is public (`isActive`).
pub fn experience_public(cookie: &SessionCookie, universe_id: u64) -> Result<bool, CloudError> {
    public_at(DEVELOP, cookie, universe_id)
}

/// Makes the experience public or private. Going private drops everyone in
/// its servers.
pub fn set_experience_public(
    cookie: &SessionCookie,
    universe_id: u64,
    public: bool,
) -> Result<(), CloudError> {
    set_public_at(DEVELOP, cookie, universe_id, public)
}

/// Whether Studio's scripts may use DataStores and the other API services.
pub fn studio_api_access(cookie: &SessionCookie, universe_id: u64) -> Result<bool, CloudError> {
    api_access_at(DEVELOP, cookie, universe_id)
}

pub fn set_studio_api_access(
    cookie: &SessionCookie,
    universe_id: u64,
    on: bool,
) -> Result<(), CloudError> {
    set_api_access_at(DEVELOP, cookie, universe_id, on)
}

/// The experience's genre and subgenre ids (`obby_and_platformer`,
/// `tower_obby`), `None` where unset. Read only: Studio shows it the same way
/// and sends people to the Creator Hub, which allows a change once every
/// three months (creator-docs `production/publishing/experience-genres.md`).
pub fn experience_genre(
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<(Option<String>, Option<String>), CloudError> {
    genre_at(APIS, cookie, universe_id)
}

fn genre_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<(Option<String>, Option<String>), CloudError> {
    let url = format!("{root}/experience-genre-api/v1/Creator/ExperienceGenre");
    let id = universe_id.to_string();
    let query = [("universeId", id.as_str()), ("genreTaxonomyVersion", "1")];
    let body: serde_json::Value =
        serde_json::from_slice(&web::send(cookie, Method::Get, &url, &query)?)?;
    Ok(parse_genre(&body))
}

/// Studio reads the reply as two ids; the field names aren't documented,
/// so any `…genre…` string counts, the `sub` one as the subgenre.
fn parse_genre(body: &serde_json::Value) -> (Option<String>, Option<String>) {
    let mut genre = None;
    let mut subgenre = None;
    if let Some(fields) = body.as_object() {
        for (key, value) in fields {
            // "na" is what an experience that never picked one answers.
            let Some(text) = value
                .as_str()
                .filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("na"))
            else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            if key.contains("subgenre") {
                subgenre.get_or_insert_with(|| text.to_string());
            } else if key.contains("genre") {
                genre.get_or_insert_with(|| text.to_string());
            }
        }
    }
    (genre, subgenre)
}

fn public_at(root: &str, cookie: &SessionCookie, universe_id: u64) -> Result<bool, CloudError> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "isActive")]
        active: bool,
    }
    let url = format!("{root}/v1/universes/{universe_id}");
    let raw: Raw = serde_json::from_slice(&web::send(cookie, Method::Get, &url, &[])?)?;
    Ok(raw.active)
}

fn set_public_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
    public: bool,
) -> Result<(), CloudError> {
    let verb = if public { "activate" } else { "deactivate" };
    let url = format!("{root}/v1/universes/{universe_id}/{verb}");
    web::send(cookie, Method::Touch, &url, &[]).map(|_| ())
}

fn api_access_at(root: &str, cookie: &SessionCookie, universe_id: u64) -> Result<bool, CloudError> {
    let url = format!("{root}/v2/universes/{universe_id}/configuration");
    // The v2 GET has answered anyone but Studio with 403 "Not allowed"; a
    // PATCH changing nothing answers with the same configuration.
    let reply = match web::send(cookie, Method::Get, &url, &[]) {
        Err(CloudError::Refused { status: 403, .. }) => {
            web::send(cookie, Method::Patch(&serde_json::json!({})), &url, &[])?
        }
        reply => reply?,
    };
    let body: serde_json::Value = serde_json::from_slice(&reply)?;
    body.get(API_FIELD)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| CloudError::Transport(format!("{url} has no {API_FIELD}")))
}

fn set_api_access_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
    on: bool,
) -> Result<(), CloudError> {
    let url = format!("{root}/v2/universes/{universe_id}/configuration");
    let body = serde_json::json!({ API_FIELD: on });
    web::send(cookie, Method::Patch(&body), &url, &[]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::serve::serve;

    fn cookie() -> SessionCookie {
        SessionCookie::new("test-cookie")
    }

    #[test]
    fn public_is_the_universe_s_is_active() {
        let (root, served) = serve(vec![(
            200,
            "",
            r#"{"id":42,"name":"x","isActive":false,"privacyType":"Private"}"#,
        )]);
        assert!(!public_at(&root, &cookie(), 42).unwrap());
        assert!(served.join().unwrap()[0].starts_with("GET /v1/universes/42 "));
    }

    #[test]
    fn going_public_or_private_posts_activate_or_deactivate_with_no_body() {
        let (root, served) = serve(vec![(200, "", "{}"), (200, "", "{}")]);
        set_public_at(&root, &cookie(), 42, true).unwrap();
        set_public_at(&root, &cookie(), 42, false).unwrap();
        let requests = served.join().unwrap();
        assert!(requests[0].starts_with("POST /v1/universes/42/activate "));
        assert!(requests[1].starts_with("POST /v1/universes/42/deactivate "));
        assert!(requests[1].ends_with("\r\n\r\n"), "{}", requests[1]);
    }

    #[test]
    fn api_access_reads_and_writes_only_the_field_on_v2() {
        let (root, served) = serve(vec![
            (
                200,
                "",
                r#"{"studioAccessToApisAllowed":true,"isActive":false}"#,
            ),
            (200, "", "{}"),
        ]);
        assert!(api_access_at(&root, &cookie(), 42).unwrap());
        set_api_access_at(&root, &cookie(), 42, false).unwrap();
        let requests = served.join().unwrap();
        assert!(requests[0].starts_with("GET /v2/universes/42/configuration "));
        assert!(requests[1].starts_with("PATCH /v2/universes/42/configuration "));
        let body: serde_json::Value =
            serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "studioAccessToApisAllowed": false })
        );
    }

    #[test]
    fn genre_reads_both_ids_and_treats_empty_as_unset() {
        let (root, served) = serve(vec![(
            200,
            "",
            r#"{"genre":"obby_and_platformer","subgenre":"tower_obby"}"#,
        )]);
        assert_eq!(
            genre_at(&root, &cookie(), 42).unwrap(),
            (
                Some("obby_and_platformer".into()),
                Some("tower_obby".into())
            )
        );
        let request = &served.join().unwrap()[0];
        assert!(request.starts_with(
            "GET /experience-genre-api/v1/Creator/ExperienceGenre?universeId=42&genreTaxonomyVersion=1 "
        ));
        let empty = serde_json::json!({ "genre": "", "subgenre": null });
        assert_eq!(parse_genre(&empty), (None, None));
        let never_set = serde_json::json!({ "genre": "na", "subgenre": "na" });
        assert_eq!(parse_genre(&never_set), (None, None));
    }

    #[test]
    fn a_refused_get_reads_the_field_from_an_empty_patch() {
        let (root, served) = serve(vec![
            (
                403,
                "",
                r#"{"errors":[{"code":0,"message":"Not allowed"}]}"#,
            ),
            (200, "", r#"{"studioAccessToApisAllowed":true}"#),
        ]);
        assert!(api_access_at(&root, &cookie(), 42).unwrap());
        let requests = served.join().unwrap();
        assert!(requests[1].starts_with("PATCH /v2/universes/42/configuration "));
        assert!(requests[1].ends_with("\r\n\r\n{}"), "{}", requests[1]);
    }

    #[test]
    fn a_configuration_without_the_field_is_an_error_not_off() {
        let (root, served) = serve(vec![(200, "", r#"{"isActive":true}"#)]);
        let err = api_access_at(&root, &cookie(), 42).unwrap_err();
        served.join().unwrap();
        assert!(err.to_string().contains(API_FIELD), "{err}");
    }
}
