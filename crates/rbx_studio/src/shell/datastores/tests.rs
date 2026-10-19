//! Pure-logic tests for the Data Stores window.

use rbx_cloud::{CloudError, EntryWrite, MapItemWrite, Revision, StoreRef};
use serde_json::json;

use super::fetch::{describe, problem};
use super::state::{store_ref, Listing, Load, Pending, Problem, Universe, UniverseSource};
use super::view::{confirm_text, ConfirmText};

fn universe(live: bool) -> Universe {
    Universe {
        id: 4_200_000_001,
        name: Some("Pebble Kart".to_string()),
        source: if live {
            UniverseSource::Linked
        } else {
            UniverseSource::Sandbox
        },
        live,
    }
}

fn at() -> StoreRef {
    store_ref(4_200_000_001, "PlayerData", "global")
}

fn text(pending: &Pending) -> ConfirmText {
    confirm_text(pending, &universe(true))
}

fn names_experience(t: &ConfirmText) {
    assert!(t.body.contains("Pebble Kart"), "{}", t.body);
    assert!(t.body.contains("4200000001"), "{}", t.body);
    assert!(t
        .details
        .iter()
        .any(|(l, v)| *l == "Experience" && v == "Pebble Kart (4200000001)"));
}

fn has_detail(t: &ConfirmText, label: &str, value: &str) -> bool {
    t.details.iter().any(|(l, v)| *l == label && v == value)
}

#[test]
fn save_entry_names_key_store_and_experience() {
    let t = text(&Pending::SaveEntry {
        at: at(),
        key: "player_1".into(),
        write: EntryWrite::default(),
        creating: false,
    });
    assert_eq!(t.title, "Save “player_1”?");
    assert_eq!(t.action, "Save");
    assert!(!t.danger);
    assert!(t.body.contains("PlayerData"));
    assert!(has_detail(&t, "Key", "player_1"));
    assert!(has_detail(&t, "Data store", "PlayerData"));
    names_experience(&t);
}

#[test]
fn creating_an_entry_says_create() {
    let t = text(&Pending::SaveEntry {
        at: at(),
        key: "k".into(),
        write: EntryWrite {
            value: json!({"a": 1}),
            ..EntryWrite::default()
        },
        creating: true,
    });
    assert_eq!(t.title, "Create “k”?");
    assert_eq!(t.action, "Create");
}

#[test]
fn delete_entry_is_danger_and_mentions_versions() {
    let t = text(&Pending::DeleteEntry {
        at: at(),
        key: "player_2".into(),
    });
    assert_eq!(t.title, "Delete “player_2”?");
    assert!(t.danger);
    assert!(t.body.contains("30 days"));
    names_experience(&t);
}

#[test]
fn increment_entry_shows_amount() {
    let t = text(&Pending::IncrementEntry {
        at: at(),
        key: "k".into(),
        amount: 5,
    });
    assert_eq!(t.title, "Increment “k” by 5?");
    assert!(has_detail(&t, "Value", "+5"));
    assert!(!t.danger);
    names_experience(&t);
}

#[test]
fn rollback_names_key_and_is_not_danger() {
    let t = text(&Pending::Rollback {
        at: at(),
        key: "k".into(),
        revision: Revision {
            revision_id: "r1".into(),
            revision_create_time: Some("2026-10-19T21:50:00Z".into()),
            etag: "e".into(),
            deleted: false,
        },
        current_etag: "e2".into(),
    });
    assert_eq!(t.title, "Roll back “k”?");
    assert!(t.body.contains("2026-10-19 21:50 UTC"));
    assert!(t.body.contains("nothing is lost"));
    assert!(!t.danger);
    names_experience(&t);
}

#[test]
fn ordered_writes_name_the_ordered_store() {
    let save = text(&Pending::SaveOrdered {
        at: at(),
        key: "alice".into(),
        value: 42,
        creating: true,
    });
    assert_eq!(save.title, "Create “alice”?");
    assert!(has_detail(&save, "Ordered data store", "PlayerData"));
    assert!(has_detail(&save, "Value", "42"));
    names_experience(&save);

    let inc = text(&Pending::IncrementOrdered {
        at: at(),
        key: "alice".into(),
        amount: -3,
    });
    assert!(has_detail(&inc, "Value", "-3"));
    assert!(!inc.danger);

    let del = text(&Pending::DeleteOrdered {
        at: at(),
        key: "alice".into(),
    });
    assert!(del.danger);
    names_experience(&del);
}

#[test]
fn map_writes_name_the_map_and_key() {
    let save = text(&Pending::SaveMapItem {
        map: "Cooldowns".into(),
        key: "boost".into(),
        write: MapItemWrite {
            value: json!(1),
            ttl_seconds: 60,
            sort_key: None,
            etag: None,
        },
        creating: false,
    });
    assert!(save.body.contains("Cooldowns"));
    assert!(has_detail(&save, "Sorted map", "Cooldowns"));
    assert!(has_detail(&save, "Key", "boost"));
    assert!(!save.danger);
    names_experience(&save);

    let del = text(&Pending::DeleteMapItem {
        map: "Cooldowns".into(),
        key: "boost".into(),
    });
    assert!(del.danger);
    assert!(del.body.contains("Cooldowns"));
    names_experience(&del);
}

#[test]
fn add_queue_item_names_queue() {
    let t = text(&Pending::AddQueueItem {
        queue: "TradeRequests".into(),
        data: json!({"from": 1}),
        priority: 0.0,
        ttl_seconds: 30,
    });
    assert_eq!(t.title, "Add an item to “TradeRequests”?");
    assert_eq!(t.action, "Add");
    assert!(has_detail(&t, "Queue", "TradeRequests"));
    names_experience(&t);
}

#[test]
fn read_queue_explains_hiding_from_game_servers() {
    let t = text(&Pending::ReadQueue {
        queue: "TradeRequests".into(),
        count: 10,
        invisibility_seconds: 30,
    });
    assert_eq!(t.title, "Read 10 items from “TradeRequests”?");
    assert!(t.body.contains("hides the items from game servers"));
    assert!(!t.danger);
    names_experience(&t);
}

#[test]
fn discard_queue_is_danger_and_counts_items() {
    let t = text(&Pending::DiscardQueue {
        queue: "TradeRequests".into(),
        read_id: "r".into(),
        count: 3,
    });
    assert_eq!(t.title, "Discard 3 queue items?");
    assert!(t.danger);
    assert!(t.body.contains("TradeRequests"));
    names_experience(&t);

    let one = text(&Pending::DiscardQueue {
        queue: "q".into(),
        read_id: "r".into(),
        count: 1,
    });
    assert_eq!(one.title, "Discard 1 queue item?");
}

#[test]
fn live_and_sandbox_give_the_same_text() {
    let pending = Pending::DeleteEntry {
        at: at(),
        key: "player_1".into(),
    };
    assert_eq!(
        confirm_text(&pending, &universe(true)),
        confirm_text(&pending, &universe(false))
    );
}

#[test]
fn unnamed_experience_falls_back_to_its_id() {
    let mut u = universe(false);
    u.name = None;
    let t = confirm_text(
        &Pending::DeleteEntry {
            at: at(),
            key: "k".into(),
        },
        &u,
    );
    assert!(t.body.contains("universe 4200000001"));
    assert!(has_detail(&t, "Experience", "Universe 4200000001"));
}

#[test]
fn default_listing_is_empty_and_idle() {
    let list: Listing<String> = Listing::default();
    assert!(list.items.is_empty());
    assert!(list.next.is_none());
    assert_eq!(list.load, Load::Idle);
    assert!(list.selected.is_none());
}

#[test]
fn missing_scope_problem_names_the_scope() {
    let err = CloudError::MissingScope {
        scope: "universe-datastores.objects:read".into(),
        message: "denied".into(),
    };
    let p = problem(&err);
    assert_eq!(
        p,
        Problem::MissingScope {
            scope: "universe-datastores.objects:read".into()
        }
    );
    assert!(describe(&p).contains("universe-datastores.objects:read"));
}
