//! Creating a new experience, as Studio's Publish to Roblox As does: the
//! universe and its start place come from `apis` `POST
//! /universes/v1/universes/create` (Studio's `placesCreateUniverse`, the
//! Creator Hub's `createUniverse`), run as the web sign-in since Open Cloud
//! has no route that creates one. The group, if any, rides in the query.
//! The new place is a copy of the Baseplate template until the caller
//! publishes the open file over it.
//!
//! Who can own it comes from `creator-home-api` `GET /v1/groups`, which
//! filters the user's groups by the permission a surface needs; Studio asks
//! for `StudioPublishNewPlace`.

use serde::Deserialize;

use crate::error::CloudError;
use crate::heartbeats::SessionCookie;
use crate::web::{self, Method, DEVELOP};

const APIS: &str = "https://apis.roblox.com";
/// Roblox's Baseplate template, what Studio's New › Baseplate opens.
const TEMPLATE_PLACE_ID: u64 = 95206881;

/// A group the user may create experiences in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatorGroup {
    pub id: u64,
    pub name: String,
}

/// What Roblox made: the universe and its start place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedExperience {
    pub universe_id: u64,
    pub root_place_id: u64,
}

/// The groups Studio's Publish As offers as owners.
pub fn creator_groups(cookie: &SessionCookie) -> Result<Vec<CreatorGroup>, CloudError> {
    groups_at(APIS, cookie)
}

/// Creates a private experience owned by `group` or the user. It is
/// named after the template until [`name_experience`].
pub fn create_experience(
    cookie: &SessionCookie,
    group: Option<u64>,
) -> Result<CreatedExperience, CloudError> {
    create_at(APIS, cookie, group)
}

/// Gives a new experience and its start place `name` and `description`.
/// Separate from the create so a refusal here can't hide that the
/// experience exists.
pub fn name_experience(
    cookie: &SessionCookie,
    created: CreatedExperience,
    name: &str,
    description: &str,
) -> Result<(), CloudError> {
    describe_at(DEVELOP, cookie, created, name, description)
}

fn groups_at(root: &str, cookie: &SessionCookie) -> Result<Vec<CreatorGroup>, CloudError> {
    #[derive(Deserialize)]
    struct Group {
        id: u64,
        name: String,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        groups: Vec<Group>,
    }
    let url = format!("{root}/creator-home-api/v1/groups");
    let query = [("surface", "StudioPublishNewPlace")];
    let page: Page = serde_json::from_slice(&web::send(cookie, Method::Get, &url, &query)?)?;
    Ok(page
        .groups
        .into_iter()
        .map(|g| CreatorGroup {
            id: g.id,
            name: g.name,
        })
        .collect())
}

fn create_at(
    root: &str,
    cookie: &SessionCookie,
    group: Option<u64>,
) -> Result<CreatedExperience, CloudError> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "universeId")]
        universe_id: u64,
        #[serde(rename = "rootPlaceId")]
        root_place_id: u64,
    }
    let url = format!("{root}/universes/v1/universes/create");
    let group = group.map(|id| id.to_string());
    let query: Vec<(&str, &str)> = group.iter().map(|id| ("groupId", id.as_str())).collect();
    let body = serde_json::json!({ "templatePlaceId": TEMPLATE_PLACE_ID, "isPublish": false });
    let raw: Raw = serde_json::from_slice(&web::send(cookie, Method::Post(&body), &url, &query)?)?;
    Ok(CreatedExperience {
        universe_id: raw.universe_id,
        root_place_id: raw.root_place_id,
    })
}

/// Names the experience and its start place: the universe through
/// `PATCH /v2/universes/{id}/configuration`, the place through
/// `PATCH /v2/places/{id}`, the two Studio's Basic Info writes.
fn describe_at(
    root: &str,
    cookie: &SessionCookie,
    created: CreatedExperience,
    name: &str,
    description: &str,
) -> Result<(), CloudError> {
    let body = serde_json::json!({ "name": name, "description": description });
    let universe = format!("{root}/v2/universes/{}/configuration", created.universe_id);
    match web::send(cookie, Method::Patch(&body), &universe, &[]) {
        // v2 has refused anyone but Studio before; v1 takes the name, and
        // the start place's description below is the experience's.
        Err(CloudError::Refused { status: 403, .. }) => {
            let v1 = format!("{root}/v1/universes/{}/configuration", created.universe_id);
            web::send(
                cookie,
                Method::Patch(&serde_json::json!({ "name": name })),
                &v1,
                &[],
            )?;
        }
        reply => {
            reply?;
        }
    }
    let place = format!("{root}/v2/places/{}", created.root_place_id);
    web::send(cookie, Method::Patch(&body), &place, &[]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::serve::serve;

    fn cookie() -> SessionCookie {
        SessionCookie::new("test-cookie")
    }

    fn json(request: &str) -> serde_json::Value {
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[test]
    fn groups_ask_for_the_publish_new_place_surface() {
        let (root, served) = serve(vec![(
            200,
            "",
            r#"{"groups":[{"id":5,"name":"Builders","roleSetName":"Owner","createdAt":"2020-01-01"}]}"#,
        )]);
        let groups = groups_at(&root, &cookie()).unwrap();
        let request = &served.join().unwrap()[0];
        assert!(
            request.starts_with("GET /creator-home-api/v1/groups?surface=StudioPublishNewPlace "),
            "{request}"
        );
        assert_eq!(
            groups,
            [CreatorGroup {
                id: 5,
                name: "Builders".into()
            }]
        );
    }

    #[test]
    fn a_user_owned_create_sends_the_template_and_no_group() {
        let (root, served) = serve(vec![(200, "", r#"{"universeId":11,"rootPlaceId":22}"#)]);
        let created = create_at(&root, &cookie(), None).unwrap();
        let request = &served.join().unwrap()[0];
        assert!(
            request.starts_with("POST /universes/v1/universes/create "),
            "{request}"
        );
        assert_eq!(
            json(request),
            serde_json::json!({ "templatePlaceId": TEMPLATE_PLACE_ID, "isPublish": false })
        );
        assert_eq!(
            created,
            CreatedExperience {
                universe_id: 11,
                root_place_id: 22
            }
        );
    }

    #[test]
    fn a_group_create_puts_the_group_in_the_query() {
        let (root, served) = serve(vec![(200, "", r#"{"universeId":11,"rootPlaceId":22}"#)]);
        create_at(&root, &cookie(), Some(5)).unwrap();
        let request = &served.join().unwrap()[0];
        assert!(
            request.starts_with("POST /universes/v1/universes/create?groupId=5 "),
            "{request}"
        );
    }

    #[test]
    fn naming_patches_the_universe_then_its_start_place() {
        let (root, served) = serve(vec![(200, "", "{}"), (200, "", "{}")]);
        let created = CreatedExperience {
            universe_id: 11,
            root_place_id: 22,
        };
        describe_at(&root, &cookie(), created, "My game", "Hi").unwrap();
        let requests = served.join().unwrap();
        assert!(requests[0].starts_with("PATCH /v2/universes/11/configuration "));
        assert!(requests[1].starts_with("PATCH /v2/places/22 "));
        let expected = serde_json::json!({ "name": "My game", "description": "Hi" });
        assert_eq!(json(&requests[0]), expected);
        assert_eq!(json(&requests[1]), expected);
    }

    #[test]
    fn a_refused_v2_name_goes_through_v1() {
        let (root, served) = serve(vec![(403, "", "{}"), (200, "", "{}"), (200, "", "{}")]);
        let created = CreatedExperience {
            universe_id: 11,
            root_place_id: 22,
        };
        describe_at(&root, &cookie(), created, "My game", "Hi").unwrap();
        let requests = served.join().unwrap();
        assert!(requests[1].starts_with("PATCH /v1/universes/11/configuration "));
        assert_eq!(json(&requests[1]), serde_json::json!({ "name": "My game" }));
        assert!(requests[2].starts_with("PATCH /v2/places/22 "));
    }

    #[test]
    fn a_refused_create_carries_roblox_s_message() {
        let (root, served) = serve(vec![(
            403,
            "",
            r#"{"errors":[{"code":0,"message":"You have reached the limit"}]}"#,
        )]);
        let err = create_at(&root, &cookie(), None).unwrap_err();
        served.join().unwrap();
        assert!(err.to_string().contains("reached the limit"), "{err}");
    }
}
