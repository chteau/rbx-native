use super::tests::{client, missing_scope, one, serve};
use super::*;
use serde_json::json;

const MAP: &str = "/cloud/v2/universes/7/memory-store/sorted-maps/m/items";
const QUEUE: &str = "/cloud/v2/universes/7/memory-store/queues/q/items";
const ITEM: &str = r#"{"path":"p","value":{"a":1},"etag":"E","ttl":"3600s","expireTime":"2026-01-01T00:00:00Z","id":"k","stringSortKey":"abc"}"#;

fn write(sort_key: Option<SortKey>, etag: Option<&str>) -> MapItemWrite {
    MapItemWrite {
        value: json!({"a": 1}),
        ttl_seconds: 3600,
        sort_key,
        etag: etag.map(str::to_string),
    }
}

#[test]
fn list_map_items_ascending_after() {
    let body = r#"{"items":[{"path":"p","value":1,"etag":"E","ttl":"60s","id":"b","numericSortKey":2.5},{"path":"p","value":"v","etag":"F","id":"c"}],"nextPageToken":"nx"}"#;
    let (root, log) = serve(vec![(200, body)]);
    let page = client(&root)
        .list_map_items(7, "m", false, Some("a"), Some("tk"))
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), MAP);
    let q = c.query();
    assert!(q.contains("maxPageSize=100"), "{q}");
    assert!(q.contains("pageToken=tk"), "{q}");
    assert!(q.contains("filter=id > \"a\""), "{q}");
    assert!(!q.contains("orderBy"), "{q}");
    assert_eq!(page.next_page_token.as_deref(), Some("nx"));
    assert_eq!(page.items[0].key, "b");
    assert_eq!(page.items[0].sort_key, Some(SortKey::Number(2.5)));
    assert_eq!(page.items[1].value, json!("v"));
    assert_eq!(page.items[1].sort_key, None);
}

#[test]
fn list_map_items_descending_after() {
    let (root, log) = serve(vec![(200, "{}")]);
    let page = client(&root)
        .list_map_items(7, "m", true, Some("a"), None)
        .unwrap();
    assert!(page.items.is_empty());
    let q = one(&log).query();
    assert!(q.contains("orderBy=id desc"), "{q}");
    assert!(q.contains("filter=id < \"a\""), "{q}");
}

#[test]
fn get_map_item_parses_every_field() {
    let (root, log) = serve(vec![(200, ITEM)]);
    let item = client(&root).get_map_item(7, "m", "k").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{MAP}/k"));
    assert_eq!(item.key, "k");
    assert_eq!(item.value, json!({"a": 1}));
    assert_eq!(item.etag, "E");
    assert_eq!(item.expire_time.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(item.sort_key, Some(SortKey::Text("abc".into())));
}

#[test]
fn create_map_item_sends_ttl_and_string_sort_key() {
    let (root, log) = serve(vec![(200, ITEM)]);
    client(&root)
        .create_map_item(7, "m", "k", &write(Some(SortKey::Text("abc".into())), None))
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), MAP);
    assert!(c.query().contains("id=k"), "{}", c.query());
    assert_eq!(
        c.json(),
        json!({"value": {"a": 1}, "ttl": "3600s", "stringSortKey": "abc"})
    );
}

#[test]
fn update_map_item_sends_numeric_sort_key_and_etag() {
    let (root, log) = serve(vec![(200, ITEM)]);
    client(&root)
        .update_map_item(7, "m", "k", &write(Some(SortKey::Number(4.0)), Some("E1")))
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "PATCH");
    assert_eq!(c.path(), format!("{MAP}/k"));
    let b = c.json();
    assert_eq!(b["numericSortKey"], json!(4.0));
    assert_eq!(b["etag"], json!("E1"));
    assert_eq!(b["ttl"], json!("3600s"));
    assert!(b.get("stringSortKey").is_none());
}

#[test]
fn update_map_item_omits_etag_and_sort_key_when_none() {
    let (root, log) = serve(vec![(200, ITEM)]);
    client(&root)
        .update_map_item(7, "m", "k", &write(None, None))
        .unwrap();
    let b = one(&log).json();
    assert!(b.get("etag").is_none());
    assert!(b.get("stringSortKey").is_none() && b.get("numericSortKey").is_none());
}

#[test]
fn delete_map_item_sends_delete() {
    let (root, log) = serve(vec![(200, "{}")]);
    client(&root).delete_map_item(7, "m", "a b").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "DELETE");
    assert_eq!(c.path(), format!("{MAP}/a%20b"));
}

#[test]
fn map_403_names_read_and_write_scopes() {
    let (root, _log) = serve(vec![(403, "{}"), (403, "{}"), (403, "{}")]);
    let c = client(&root);
    assert_eq!(
        missing_scope(c.get_map_item(7, "m", "k").unwrap_err()),
        "memory-store.sorted-map:read"
    );
    assert_eq!(
        missing_scope(c.delete_map_item(7, "m", "k").unwrap_err()),
        "memory-store.sorted-map:write"
    );
    assert_eq!(
        missing_scope(
            c.create_map_item(7, "m", "k", &write(None, None))
                .unwrap_err()
        ),
        "memory-store.sorted-map:write"
    );
}

#[test]
fn read_queue_sends_window_and_parses_items() {
    let body = r#"{"readId":"R1","items":[{"path":"p","data":{"job":1},"priority":2,"ttl":"60s","expireTime":"x","id":"i1"}]}"#;
    let (root, log) = serve(vec![(200, body)]);
    let read = client(&root).read_queue(7, "q", 10, 30).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{QUEUE}:read"));
    let q = c.query();
    assert!(q.contains("count=10"), "{q}");
    assert!(q.contains("allOrNothing=false"), "{q}");
    assert!(q.contains("invisibilityWindow=30s"), "{q}");
    assert_eq!(read.read_id.as_deref(), Some("R1"));
    assert_eq!(
        read.items,
        vec![QueueItem {
            id: "i1".into(),
            data: json!({"job": 1}),
            priority: 2.0,
            expire_time: Some("x".into())
        }]
    );
}

#[test]
fn read_queue_empty_has_no_read_id() {
    let (root, _log) = serve(vec![(200, "{}")]);
    let read = client(&root).read_queue(7, "q", 1, 5).unwrap();
    assert_eq!(read.read_id, None);
    assert!(read.items.is_empty());
}

#[test]
fn discard_queue_items_posts_read_id() {
    let (root, log) = serve(vec![(200, "{}")]);
    client(&root).discard_queue_items(7, "q", "R1").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), format!("{QUEUE}:discard"));
    assert_eq!(c.json(), json!({"readId": "R1"}));
}

#[test]
fn add_queue_item_posts_data_priority_ttl() {
    let reply = r#"{"path":"p","data":"hi","priority":1.5,"ttl":"90s","expireTime":"e","id":"n1"}"#;
    let (root, log) = serve(vec![(200, reply)]);
    let item = client(&root)
        .add_queue_item(7, "q", &json!("hi"), 1.5, 90)
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), QUEUE);
    assert_eq!(
        c.json(),
        json!({"data": "hi", "priority": 1.5, "ttl": "90s"})
    );
    assert_eq!(item.id, "n1");
    assert_eq!(item.priority, 1.5);
}

#[test]
fn queue_403_names_scopes() {
    let (root, _log) = serve(vec![(403, "{}"), (403, "{}"), (403, "{}")]);
    let c = client(&root);
    assert_eq!(
        missing_scope(c.read_queue(7, "q", 1, 1).unwrap_err()),
        "memory-store.queue:dequeue"
    );
    assert_eq!(
        missing_scope(c.discard_queue_items(7, "q", "r").unwrap_err()),
        "memory-store.queue:discard"
    );
    assert_eq!(
        missing_scope(c.add_queue_item(7, "q", &json!(1), 0.0, 1).unwrap_err()),
        "memory-store.queue:add"
    );
}
