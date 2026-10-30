//! Security's "Allow Mesh / Image APIs" switch: whether published servers
//! and clients may create `EditableMesh` and `EditableImage`. Open Cloud has
//! no field for it, so it runs as the web sign-in (see `web`).
//!
//! Studio's Game Settings plugin keeps it as `isMeshTextureApiAccessAllowed`
//! on `develop.roblox.com/v2/universes/{id}/configuration`, but that
//! endpoint answers anyone but Studio with 403 "Not allowed". The v1
//! `configuration` carries the same field both ways, and it's what the
//! Creator Hub's own API client reads and patches, so that's the one used.

use serde::Deserialize;

use crate::error::CloudError;
use crate::heartbeats::{self, PulseError, SessionCookie};
use crate::web::{self, Method, DEVELOP};

const GROUPS: &str = "https://groups.roblox.com";
const FIELD: &str = "isMeshTextureApiAccessAllowed";

/// The switch as Studio shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshImageApis {
    pub allowed: bool,
    /// False when a group owns the experience and the signed-in user isn't
    /// the group's owner: Studio greys the switch out then.
    pub can_edit: bool,
}

/// Reads the switch and whether the signed-in user may flip it.
pub fn mesh_image_apis(
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<MeshImageApis, CloudError> {
    let allowed = allowed_at(DEVELOP, cookie, universe_id)?;
    let can_edit = match group_owner_at(DEVELOP, GROUPS, cookie, universe_id)? {
        None => true,
        Some(owner) => match heartbeats::authenticated_user_id(cookie) {
            Ok(user) => user == owner,
            Err(PulseError::Unauthorized) => {
                return Err(CloudError::Refused {
                    status: 401,
                    message: "Authorization has been denied for this request.".into(),
                })
            }
            Err(e) => return Err(CloudError::Transport(format!("users/authenticated: {e:?}"))),
        },
    };
    Ok(MeshImageApis { allowed, can_edit })
}

/// Turns the switch on or off
/// (`PATCH /v1/universes/{id}/configuration {"isMeshTextureApiAccessAllowed":…}`).
pub fn set_mesh_image_apis_allowed(
    cookie: &SessionCookie,
    universe_id: u64,
    on: bool,
) -> Result<(), CloudError> {
    set_allowed_at(DEVELOP, cookie, universe_id, on)
}

fn allowed_at(root: &str, cookie: &SessionCookie, universe_id: u64) -> Result<bool, CloudError> {
    let url = format!("{root}/v1/universes/{universe_id}/configuration");
    let body: serde_json::Value =
        serde_json::from_slice(&web::send(cookie, Method::Get, &url, &[])?)?;
    body.get(FIELD)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| CloudError::Transport(format!("{url} has no {FIELD}")))
}

fn set_allowed_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
    on: bool,
) -> Result<(), CloudError> {
    let url = format!("{root}/v1/universes/{universe_id}/configuration");
    let body = serde_json::json!({ FIELD: on });
    web::send(cookie, Method::Patch(&body), &url, &[]).map(|_| ())
}

/// The owning group's owner, or `None` for a user-owned experience.
fn group_owner_at(
    develop: &str,
    groups: &str,
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<Option<u64>, CloudError> {
    #[derive(Deserialize)]
    struct Universe {
        #[serde(rename = "creatorType")]
        creator_type: String,
        #[serde(rename = "creatorTargetId")]
        creator_id: u64,
    }
    #[derive(Deserialize)]
    struct Page {
        data: Vec<Universe>,
    }
    #[derive(Deserialize)]
    struct Owner {
        #[serde(rename = "userId")]
        user_id: u64,
    }
    #[derive(Deserialize)]
    struct Group {
        owner: Option<Owner>,
    }
    let url = format!("{develop}/v1/universes/multiget");
    let id = universe_id.to_string();
    let page: Page =
        serde_json::from_slice(&web::send(cookie, Method::Get, &url, &[("ids", &id)])?)?;
    let Some(universe) = page.data.into_iter().next() else {
        return Err(CloudError::Transport(format!(
            "Roblox has no universe {universe_id}"
        )));
    };
    if universe.creator_type != "Group" {
        return Ok(None);
    }
    let url = format!("{groups}/v1/groups/{}", universe.creator_id);
    let group: Group = serde_json::from_slice(&web::send(cookie, Method::Get, &url, &[])?)?;
    // An ownerless group has nobody who can flip it.
    Ok(Some(group.owner.map_or(0, |o| o.user_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::serve::serve;

    fn cookie() -> SessionCookie {
        SessionCookie::new("test-cookie")
    }

    #[test]
    fn reads_the_field_from_the_v1_configuration_with_the_cookie() {
        let (root, served) = serve(vec![(
            200,
            "",
            r#"{"studioAccessToApisAllowed":false,"isMeshTextureApiAccessAllowed":true}"#,
        )]);
        assert!(allowed_at(&root, &cookie(), 42).unwrap());
        let request = &served.join().unwrap()[0];
        assert!(
            request.starts_with("GET /v1/universes/42/configuration "),
            "{request}"
        );
        assert!(request
            .to_lowercase()
            .contains("cookie: .roblosecurity=test-cookie"));
    }

    #[test]
    fn a_reply_without_the_field_is_an_error_not_off() {
        let (root, served) = serve(vec![(200, "", r#"{"studioAccessToApisAllowed":false}"#)]);
        let err = allowed_at(&root, &cookie(), 42).unwrap_err();
        served.join().unwrap();
        assert!(err.to_string().contains(FIELD), "{err}");
    }

    #[test]
    fn writing_patches_only_the_field_after_the_csrf_retry() {
        let (root, served) = serve(vec![
            (
                403,
                "x-csrf-token: fresh\r\n",
                r#"{"errors":[{"code":0,"message":"Token Validation Failed"}]}"#,
            ),
            (200, "", "{}"),
        ]);
        set_allowed_at(&root, &cookie(), 42, true).unwrap();
        let requests = served.join().unwrap();
        let last = &requests[1];
        assert!(
            last.starts_with("PATCH /v1/universes/42/configuration "),
            "{last}"
        );
        assert!(last.to_lowercase().contains("x-csrf-token: fresh"));
        let body: serde_json::Value =
            serde_json::from_str(last.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "isMeshTextureApiAccessAllowed": true })
        );
    }

    #[test]
    fn a_user_owned_experience_has_no_group_owner() {
        let (root, served) = serve(vec![(
            200,
            "",
            r#"{"data":[{"id":42,"creatorType":"User","creatorTargetId":7}]}"#,
        )]);
        assert_eq!(group_owner_at(&root, &root, &cookie(), 42).unwrap(), None);
        let requests = served.join().unwrap();
        assert!(requests[0].starts_with("GET /v1/universes/multiget?ids=42 "));
    }

    #[test]
    fn a_group_owned_experience_answers_the_group_s_owner() {
        let (root, served) = serve(vec![
            (
                200,
                "",
                r#"{"data":[{"id":42,"creatorType":"Group","creatorTargetId":5}]}"#,
            ),
            (200, "", r#"{"id":5,"owner":{"userId":99,"username":"x"}}"#),
        ]);
        assert_eq!(
            group_owner_at(&root, &root, &cookie(), 42).unwrap(),
            Some(99)
        );
        let requests = served.join().unwrap();
        assert!(
            requests[1].starts_with("GET /v1/groups/5 "),
            "{}",
            requests[1]
        );
    }
}
