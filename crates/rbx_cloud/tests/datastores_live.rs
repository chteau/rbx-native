//! Live round trip of every data store call against real Roblox. Ignored by
//! default: it writes, so it only runs against a store whose name says it is
//! a throwaway, in a universe named explicitly.
//!
//! ```sh
//! RBX_API_KEY=... RBX_DATASTORE_LIVE_UNIVERSE=<sandbox universe id> \
//! RBX_DATASTORE_LIVE_STORE=rbxnative-e2e-<date> \
//! [RBX_DATASTORE_LIVE_ROLLBACK_KEY=<key in that store with two revisions>] \
//! cargo test -p rbx_cloud --test datastores_live -- --ignored --nocapture
//! ```
//!
//! Everything it creates it deletes again. The memory store half accepts a
//! key without the memory store permissions, as long as the refusal names the
//! missing scope.

use rbx_cloud::{
    ApiKey, Client, CloudError, EntryWrite, MapItemWrite, OrderedQuery, SortKey, StoreRef,
    ALL_SCOPES,
};
use serde_json::json;

const PREFIX: &str = "rbxnative-e2e-";

fn setup() -> Option<(Client, u64, String)> {
    let key = ApiKey::from_env_or_config()?;
    let universe = std::env::var("RBX_DATASTORE_LIVE_UNIVERSE")
        .ok()?
        .parse()
        .ok()?;
    let store = std::env::var("RBX_DATASTORE_LIVE_STORE").ok()?;
    assert!(
        store.starts_with(PREFIX),
        "refusing to write to {store}: the store name must start with {PREFIX}"
    );
    Some((Client::new(Some(key)), universe, store))
}

#[test]
#[ignore]
fn standard_entries_round_trip() {
    let Some((client, universe_id, store)) = setup() else {
        eprintln!("RBX_DATASTORE_LIVE_UNIVERSE / _STORE unset, skipping");
        return;
    };
    let at = StoreRef {
        universe_id,
        store: store.clone(),
        scope: "global".into(),
    };
    let key = format!("e2e-{}", std::process::id());
    let write = EntryWrite {
        value: json!({"coins": 5, "inventory": [{"id": 1}]}),
        users: vec![925_308_243],
        attributes: serde_json::from_value(json!({"schema": 1})).unwrap(),
        etag: None,
    };
    let created = client.create_entry(&at, &key, &write).expect("create");
    assert_eq!(created.value, write.value);
    assert_eq!(created.users, write.users);

    let read = client.get_entry(&at, &key).expect("get");
    assert_eq!(read.etag, created.etag);

    let updated = client
        .update_entry(
            &at,
            &key,
            &EntryWrite {
                value: json!({"coins": 6}),
                etag: Some(read.etag.clone()),
                ..write.clone()
            },
        )
        .expect("update with the current etag");
    assert_eq!(updated.value, json!({"coins": 6}));

    let stale = client.update_entry(
        &at,
        &key,
        &EntryWrite {
            value: json!({"coins": 7}),
            etag: Some(read.etag.clone()),
            ..write.clone()
        },
    );
    assert!(
        matches!(stale, Err(CloudError::Conflict { .. })),
        "{stale:?}"
    );

    let counter = format!("{key}-n");
    client
        .create_entry(
            &at,
            &counter,
            &EntryWrite {
                value: json!(10),
                ..EntryWrite::default()
            },
        )
        .expect("create counter");
    let bumped = client
        .increment_entry(&at, &counter, 5, &[], &Default::default())
        .expect("increment");
    assert_eq!(bumped.value, json!(15));

    let mut found = Vec::new();
    let mut token = None;
    loop {
        let page = client
            .list_entries(&at, "e2e-", token.as_deref())
            .expect("list keys");
        found.extend(page.items.into_iter().map(|e| e.key));
        token = page.next_page_token;
        if token.is_none() {
            break;
        }
    }
    assert!(found.contains(&key), "{found:?}");

    let everywhere = StoreRef {
        scope: ALL_SCOPES.into(),
        ..at.clone()
    };
    let mut scoped = Vec::new();
    let mut token = None;
    loop {
        let page = client
            .list_entries(&everywhere, "", token.as_deref())
            .expect("list keys in every scope");
        scoped.extend(page.items);
        token = page.next_page_token;
        if token.is_none() {
            break;
        }
    }
    assert!(scoped.iter().any(|e| e.key == key && e.scope == "global"));

    let revisions = client
        .list_revisions(&at, &key, None)
        .expect("list revisions");
    assert!(!revisions.items.is_empty());
    let at_revision = client
        .get_entry_revision(&at, &key, &revisions.items[0].revision_id)
        .expect("get revision");
    assert_eq!(at_revision.value, json!({"coins": 6}));

    if let Ok(rollback_key) = std::env::var("RBX_DATASTORE_LIVE_ROLLBACK_KEY") {
        let mut all = Vec::new();
        let mut token = None;
        loop {
            let page = client
                .list_revisions(&at, &rollback_key, token.as_deref())
                .expect("list rollback revisions");
            all.extend(page.items);
            token = page.next_page_token;
            if token.is_none() {
                break;
            }
        }
        assert!(all.len() >= 2, "{rollback_key} needs two revisions");
        let oldest = all.last().unwrap();
        let old = client
            .get_entry_revision(&at, &rollback_key, &oldest.revision_id)
            .expect("read the oldest revision");
        let current = client.get_entry(&at, &rollback_key).expect("current");
        let rolled = client
            .rollback_entry(&at, &rollback_key, &oldest.revision_id, &current.etag)
            .expect("rollback");
        assert_eq!(rolled.value, old.value);
        assert_ne!(rolled.revision_id, current.revision_id);
        eprintln!("rolled {rollback_key} back to {}", oldest.revision_id);
    }

    client.delete_entry(&at, &key).expect("delete");
    client.delete_entry(&at, &counter).expect("delete counter");
    assert!(client.get_entry(&at, &key).is_err());
}

#[test]
#[ignore]
fn ordered_entries_round_trip() {
    let Some((client, universe_id, store)) = setup() else {
        return;
    };
    let at = StoreRef {
        universe_id,
        store,
        scope: "global".into(),
    };
    let low = format!("e2e-low-{}", std::process::id());
    let high = format!("e2e-high-{}", std::process::id());
    client.create_ordered(&at, &low, 10).expect("create low");
    client.create_ordered(&at, &high, 30).expect("create high");
    assert_eq!(client.increment_ordered(&at, &low, 5).unwrap().value, 15);
    let page = client
        .list_ordered(
            &at,
            OrderedQuery {
                descending: true,
                min: Some(12),
                max: None,
            },
            None,
        )
        .expect("list");
    let keys: Vec<_> = page.items.iter().map(|e| e.key.as_str()).collect();
    let (hi, lo) = (
        keys.iter().position(|k| *k == high),
        keys.iter().position(|k| *k == low),
    );
    assert!(hi.is_some() && lo.is_some() && hi < lo, "{keys:?}");
    assert_eq!(client.update_ordered(&at, &high, 3).unwrap().value, 3);
    assert_eq!(client.get_ordered(&at, &high).unwrap().value, 3);
    client.delete_ordered(&at, &low).expect("delete low");
    client.delete_ordered(&at, &high).expect("delete high");
}

#[test]
#[ignore]
fn memory_store_round_trip_or_named_scope() {
    let Some((client, universe_id, store)) = setup() else {
        return;
    };
    let key = format!("e2e-{}", std::process::id());
    let write = MapItemWrite {
        value: json!({"party": [1, 2]}),
        ttl_seconds: 300,
        sort_key: Some(SortKey::Number(4.0)),
        etag: None,
    };
    match client.create_map_item(universe_id, &store, &key, &write) {
        Err(CloudError::MissingScope { scope, .. }) => {
            assert_eq!(scope, "memory-store.sorted-map:write");
            eprintln!("memory store: the key lacks {scope}");
            let read = client.read_queue(universe_id, &store, 1, 5);
            assert!(
                matches!(read, Err(CloudError::MissingScope { ref scope, .. }) if scope == "memory-store.queue:dequeue"),
                "{read:?}"
            );
        }
        Ok(item) => {
            assert_eq!(item.value, write.value);
            let listed = client
                .list_map_items(universe_id, &store, false, None, None)
                .expect("list map");
            assert!(listed.items.iter().any(|i| i.key == key));
            let got = client
                .get_map_item(universe_id, &store, &key)
                .expect("get map item");
            assert_eq!(got.sort_key, Some(SortKey::Number(4.0)));
            let updated = client
                .update_map_item(
                    universe_id,
                    &store,
                    &key,
                    &MapItemWrite {
                        value: json!({"party": [1, 2, 3]}),
                        sort_key: Some(SortKey::Text("b".into())),
                        etag: Some(got.etag.clone()),
                        ..write.clone()
                    },
                )
                .expect("update map item");
            assert_eq!(updated.value, json!({"party": [1, 2, 3]}));
            client
                .delete_map_item(universe_id, &store, &key)
                .expect("delete map item");
            client
                .add_queue_item(universe_id, &store, &json!({"n": 1}), 0.0, 300)
                .expect("add queue item");
            let read = client.read_queue(universe_id, &store, 10, 5).expect("read");
            assert!(!read.items.is_empty());
            client
                .discard_queue_items(universe_id, &store, read.read_id.as_deref().unwrap())
                .expect("discard");
        }
        Err(other) => panic!("unexpected: {other}"),
    }
}
