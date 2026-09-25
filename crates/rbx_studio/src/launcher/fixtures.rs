//! Canned answers for scripted captures of the launcher, so every screen
//! state can be shot without a network, a key, or a particular account:
//!
//! - `RBX_STUDIO_LAUNCHER_KEY=ready|missing|invalid|expired|disabled|network`
//!   answers the key check (see `key_check`); `noinventory` is `ready`
//!   without the Inventory scope, and Home reads it too.
//! - `RBX_STUDIO_LAUNCHER_GAMES=list|loading|empty` answers My Games.
//!
//! The data matches the design mock-ups, so a capture diffs against them.

use std::collections::HashMap;

use rbx_cloud::{Experience, Experiences, Group, KeyInfo, Owner, Scope, Visibility};

use super::key_check::{Checked, Status};

pub(super) const GAMES_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_GAMES";

fn scope(name: &str, operation: &str, universes: &[u64]) -> Scope {
    Scope {
        name: name.to_string(),
        operations: vec![operation.to_string()],
        universe_ids: universes.to_vec(),
    }
}

pub(super) fn key_status(which: &str) -> Status {
    let mut info = KeyInfo {
        name: "RbxNative".to_string(),
        authorized_user_id: 925308243,
        scopes: vec![
            scope("universe-places", "write", &[]),
            scope("legacy-asset", "manage", &[]),
            scope("user.inventory-item", "read", &[]),
            scope("legacy-group", "manage", &[]),
            scope("universe.place", "read", &[9828239630]),
            scope("universe", "write", &[9828239630, 9440522195]),
            scope("universe.thumbnail", "read", &[]),
        ],
        enabled: true,
        expired: false,
        expiration_time_utc: "2026-12-24T00:00:00Z".to_string(),
    };
    match which {
        "invalid" => return Status::Invalid(401),
        "network" => return Status::Network,
        "missing" => {
            info.scopes = vec![
                scope("universe-places", "write", &[]),
                scope("user.inventory-item", "read", &[]),
                scope("universe.thumbnail", "read", &[]),
            ]
        }
        "expired" => {
            info.expired = true;
            info.expiration_time_utc = "2026-09-20T00:00:00Z".to_string();
        }
        "disabled" => info.enabled = false,
        "noinventory" => info.scopes.retain(|s| s.name != "user.inventory-item"),
        "running" => return Status::Running,
        _ => {}
    }
    let report = rbx_cloud::check_scopes(&info);
    Status::Done(Box::new(Checked {
        info,
        report,
        owner: "Cheeteau".to_string(),
        universes: HashMap::from([
            (9828239630, "Fragment - Demo".to_string()),
            (9440522195, "[NEW] MARKED".to_string()),
        ]),
    }))
}

/// `None` stands for "still loading".
pub(super) fn games(which: &str) -> Option<Experiences> {
    let game = |universe_id, root_place_id, name: &str, visibility, owner| Experience {
        universe_id,
        root_place_id,
        name: name.to_string(),
        visibility,
        owner,
    };
    let me = Owner::User(925308243);
    let group = Owner::Group(35120411);
    match which {
        "loading" => None,
        "empty" => Some(Experiences::default()),
        _ => Some(Experiences {
            experiences: vec![
                game(
                    9828239630,
                    12840211733,
                    "Fragment - Demo",
                    Visibility::Public,
                    me,
                ),
                game(
                    9440522195,
                    10982334120,
                    "[NEW] MARKED",
                    Visibility::Public,
                    me,
                ),
                game(
                    6053515322,
                    15530827741,
                    "Copy of Test",
                    Visibility::Private,
                    me,
                ),
                game(
                    3401771940,
                    9211457608,
                    "Swim For Treasure",
                    Visibility::Public,
                    me,
                ),
                game(
                    5537139473,
                    16004419372,
                    "Knife Combat",
                    Visibility::Public,
                    me,
                ),
                game(5078320679, 14471190058, "Echoes", Visibility::Private, me),
                game(
                    4760001234,
                    13377020641,
                    "[BETA!] Cardverse",
                    Visibility::Public,
                    group,
                ),
                game(
                    4760001235,
                    13377101844,
                    "Cardverse Lobby",
                    Visibility::Public,
                    group,
                ),
            ],
            groups: vec![Group {
                id: 35120411,
                name: "Cardverse Community".to_string(),
            }],
        }),
    }
}
