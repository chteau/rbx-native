use serde_json::json;

use super::{start, MockOptions, MOCK_UNIVERSE_ID};
use crate::{
    ApiKey, Client, CloudError, EntryRef, EntryWrite, MapItemWrite, OrderedQuery, SortKey,
    StoreRef, ALL_SCOPES,
};

fn client_with(options: MockOptions) -> Client {
    Client::new(Some(ApiKey::new("k"))).with_apis_root(start(options))
}

fn client() -> Client {
    client_with(MockOptions::default())
}

fn at(store: &str, scope: &str) -> StoreRef {
    StoreRef {
        universe_id: MOCK_UNIVERSE_ID,
        store: store.to_owned(),
        scope: scope.to_owned(),
    }
}

fn all_keys(c: &Client, store: &StoreRef, prefix: &str) -> Vec<EntryRef> {
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = c
            .list_entries(store, prefix, token.as_deref())
            .expect("list entries");
        out.extend(page.items);
        token = page.next_page_token;
        if token.is_none() {
            return out;
        }
    }
}

fn write(value: serde_json::Value) -> EntryWrite {
    EntryWrite {
        value,
        ..EntryWrite::default()
    }
}

#[test]
fn lists_stores_and_hides_deleted_ones() {
    let c = client();
    let names: Vec<String> = c
        .list_data_stores(MOCK_UNIVERSE_ID, "", false, None)
        .expect("list")
        .items
        .into_iter()
        .map(|s| s.id)
        .collect();
    for want in ["PlayerData", "Settings", "Inventory_v3", "Bans"] {
        assert!(names.iter().any(|n| n == want), "missing {want}");
    }
    assert!(!names.iter().any(|n| n == "OldPlayerData_v1"));

    let with_deleted = c
        .list_data_stores(MOCK_UNIVERSE_ID, "Old", true, None)
        .expect("list deleted");
    assert_eq!(with_deleted.items.len(), 1);
    assert!(with_deleted.items[0].deleted);
}

#[test]
fn wildcard_listing_follows_pages_to_150_keys() {
    let c = client();
    let keys = all_keys(&c, &at("PlayerData", ALL_SCOPES), "");
    assert_eq!(keys.len(), 150);
    assert!(keys.iter().any(|k| k.scope == "season2"));
    assert_eq!(
        all_keys(&c, &at("PlayerData", "global"), "player_1000100").len(),
        1
    );
}

#[test]
fn reads_nested_player_data() {
    let c = client();
    let entry = c
        .get_entry(&at("PlayerData", "global"), "player_1000001")
        .expect("get");
    assert!(entry.value["coins"].is_u64());
    assert!(entry.value["inventory"][0]["name"].is_string());
    assert_eq!(entry.users, vec![1_000_001]);
    assert_eq!(entry.attributes["schemaVersion"], json!(3));
}

#[test]
fn create_update_and_stale_etag() {
    let c = client();
    let store = at("Settings", "global");
    c.create_entry(&store, "fresh", &write(json!({"a": 1})))
        .expect("create");
    assert!(c.create_entry(&store, "fresh", &write(json!(2))).is_err());

    let current = c.get_entry(&store, "fresh").expect("get");
    let stale = EntryWrite {
        etag: Some("stale".to_owned()),
        ..write(json!(3))
    };
    assert!(matches!(
        c.update_entry(&store, "fresh", &stale),
        Err(CloudError::Conflict { .. })
    ));

    let guarded = EntryWrite {
        etag: Some(current.etag.clone()),
        ..write(json!(4))
    };
    let updated = c.update_entry(&store, "fresh", &guarded).expect("update");
    assert_eq!(updated.value, json!(4));
    assert_ne!(updated.etag, current.etag);
}

#[test]
fn increments_and_rejects_non_numbers() {
    let c = client();
    let store = at("Settings", "global");
    let first = c
        .increment_entry(&store, "counter", 5, &[], &serde_json::Map::new())
        .expect("create by increment");
    assert_eq!(first.value, json!(5));
    let second = c
        .increment_entry(&store, "counter", 3, &[], &serde_json::Map::new())
        .expect("increment");
    assert_eq!(second.value, json!(8));
    assert!(matches!(
        c.increment_entry(&store, "motd", 1, &[], &serde_json::Map::new()),
        Err(CloudError::Refused { .. })
    ));
}

#[test]
fn revisions_roll_back_and_delete() {
    let c = client();
    let store = at("PlayerData", "global");
    let key = "player_1000002";
    let revisions = c
        .list_revisions(&store, key, None)
        .expect("revisions")
        .items;
    assert!(revisions.len() >= 2);
    assert!(revisions[0].revision_create_time > revisions[1].revision_create_time);

    let oldest = revisions.last().expect("oldest");
    let old = c
        .get_entry_revision(&store, key, &oldest.revision_id)
        .expect("old revision");
    let current = c.get_entry(&store, key).expect("current");
    let rolled = c
        .rollback_entry(&store, key, &oldest.revision_id, &current.etag)
        .expect("rollback");
    assert_eq!(rolled.value, old.value);
    assert_ne!(rolled.revision_id, current.revision_id);

    c.delete_entry(&store, key).expect("delete");
    assert!(c.get_entry(&store, key).is_err());
    assert!(c.delete_entry(&store, key).is_err());
}

#[test]
fn ordered_listing_filters_and_sorts() {
    let c = client();
    let store = at("Wins", "global");
    let query = OrderedQuery {
        descending: true,
        min: Some(10),
        max: Some(300),
    };
    let rows = c.list_ordered(&store, query, None).expect("list").items;
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| (10..=300).contains(&r.value)));
    assert!(rows.windows(2).all(|w| w[0].value >= w[1].value));

    c.create_ordered(&store, "newcomer", 7).expect("create");
    let bumped = c
        .increment_ordered(&store, "newcomer", 5)
        .expect("increment");
    assert_eq!(bumped.value, 12);
    assert_eq!(
        c.update_ordered(&store, "newcomer", 1)
            .expect("update")
            .value,
        1
    );
    c.delete_ordered(&store, "newcomer").expect("delete");
    assert!(c.get_ordered(&store, "newcomer").is_err());
}

#[test]
fn sorted_map_items_page_and_guard_etags() {
    let c = client();
    let mut seen = 0;
    let mut token: Option<String> = None;
    loop {
        let page = c
            .list_map_items(
                MOCK_UNIVERSE_ID,
                "Matchmaking",
                false,
                None,
                token.as_deref(),
            )
            .expect("list");
        seen += page.items.len();
        assert!(page
            .items
            .iter()
            .all(|i| matches!(i.sort_key, Some(SortKey::Number(_)))));
        token = page.next_page_token;
        if token.is_none() {
            break;
        }
    }
    assert_eq!(seen, 25);

    let after = c
        .list_map_items(
            MOCK_UNIVERSE_ID,
            "Matchmaking",
            false,
            Some("match_22"),
            None,
        )
        .expect("after");
    assert_eq!(after.items.len(), 2);

    let mut item = MapItemWrite {
        value: json!({"mode": "duos"}),
        ttl_seconds: 600,
        sort_key: Some(SortKey::Text("zed".to_owned())),
        etag: None,
    };
    let created = c
        .create_map_item(MOCK_UNIVERSE_ID, "Auctions", "fresh", &item)
        .expect("create");
    item.etag = Some("stale".to_owned());
    assert!(matches!(
        c.update_map_item(MOCK_UNIVERSE_ID, "Auctions", "fresh", &item),
        Err(CloudError::Conflict { .. })
    ));
    item.etag = Some(created.etag);
    c.update_map_item(MOCK_UNIVERSE_ID, "Auctions", "fresh", &item)
        .expect("update");
    c.delete_map_item(MOCK_UNIVERSE_ID, "Auctions", "fresh")
        .expect("delete");
    assert!(c
        .get_map_item(MOCK_UNIVERSE_ID, "Auctions", "fresh")
        .is_err());
}

#[test]
fn queue_read_hides_items_until_discarded() {
    let c = client();
    let first = c
        .read_queue(MOCK_UNIVERSE_ID, "TradeRequests", 5, 30)
        .expect("first read");
    assert_eq!(first.items.len(), 5);
    let second = c
        .read_queue(MOCK_UNIVERSE_ID, "TradeRequests", 200, 30)
        .expect("second read");
    assert_eq!(second.items.len(), 7);
    assert!(second
        .items
        .iter()
        .all(|s| first.items.iter().all(|f| f.id != s.id)));

    let read_id = first.read_id.expect("read id");
    c.discard_queue_items(MOCK_UNIVERSE_ID, "TradeRequests", &read_id)
        .expect("discard");
    let added = c
        .add_queue_item(MOCK_UNIVERSE_ID, "TradeRequests", &json!({"x": 1}), 9.0, 60)
        .expect("add");
    let third = c
        .read_queue(MOCK_UNIVERSE_ID, "TradeRequests", 1, 30)
        .expect("third read");
    assert_eq!(third.items[0].id, added.id);
}

#[test]
fn denied_scope_reports_missing_permission() {
    let c = client_with(MockOptions {
        denied_scopes: vec!["universe-datastores.objects:read".to_owned()],
        ..MockOptions::default()
    });
    assert!(matches!(
        c.get_entry(&at("Settings", "global"), "motd"),
        Err(CloudError::MissingScope { .. })
    ));
    assert!(c
        .list_data_stores(MOCK_UNIVERSE_ID, "", false, None)
        .is_ok());
}

#[test]
fn throttled_requests_still_succeed_after_retry() {
    let c = client_with(MockOptions {
        throttle_every: Some(2),
        ..MockOptions::default()
    });
    for _ in 0..3 {
        c.get_entry(&at("Settings", "global"), "motd").expect("get");
    }
}
