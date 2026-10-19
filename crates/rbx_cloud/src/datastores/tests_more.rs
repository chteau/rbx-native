use super::*;
use crate::{ApiKey, Client, CloudError};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct Captured {
    method: String,
    target: String,
    api_key: Option<String>,
    body: String,
}

impl Captured {
    fn path(&self) -> &str {
        self.target.split('?').next().unwrap()
    }
    fn query(&self) -> String {
        decode(self.target.split_once('?').map_or("", |(_, q)| q))
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

type Log = Arc<Mutex<Vec<Captured>>>;

fn serve(replies: Vec<(u16, &str)>) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let queue: VecDeque<(u16, String)> = replies
        .into_iter()
        .map(|(s, b)| (s, b.to_string()))
        .collect();
    let queue = Arc::new(Mutex::new(queue));
    let thread_log = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, length) = loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break (buf.len(), 0);
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (pos + 4, length);
                }
            };
            while buf.len() < head_end + length {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
            let mut lines = head.lines();
            let mut first = lines.next().unwrap_or("").split(' ');
            let method = first.next().unwrap_or("").to_string();
            let target = first.next().unwrap_or("").to_string();
            let api_key = lines.find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("x-api-key")
                    .then(|| value.trim().to_string())
            });
            let body = String::from_utf8_lossy(&buf[head_end.min(buf.len())..]).to_string();
            thread_log.lock().unwrap().push(Captured {
                method,
                target,
                api_key,
                body,
            });
            let (status, reply) = queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or((200, "{}".to_string()));
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (root, log)
}

fn client(root: &str) -> Client {
    Client::new(Some(ApiKey::new("k"))).with_apis_root(root)
}

fn at() -> StoreRef {
    StoreRef {
        universe_id: 7,
        store: "Top".into(),
        scope: "global".into(),
    }
}

fn one(log: &Log) -> Captured {
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1, "{log:?}");
    log[0].clone()
}

const OBASE: &str = "/cloud/v2/universes/7/ordered-data-stores/Top/scopes/global/entries";
const MAP: &str = "/cloud/v2/universes/7/memory-store/sorted-maps/m/items";
const QUEUE: &str = "/cloud/v2/universes/7/memory-store/queues/q/items";
const ORDERED: &str = r#"{"path":"p","value":15,"id":"p1"}"#;
const ITEM: &str = r#"{"path":"p","value":{"a":1},"etag":"E","ttl":"3600s","expireTime":"2026-01-01T00:00:00Z","id":"k","stringSortKey":"abc"}"#;

fn write(sort_key: Option<SortKey>, etag: Option<&str>) -> MapItemWrite {
    MapItemWrite {
        value: json!({"a": 1}),
        ttl_seconds: 3600,
        sort_key,
        etag: etag.map(str::to_string),
    }
}

fn missing_scope(err: CloudError) -> String {
    match err {
        CloudError::MissingScope { scope, .. } => scope,
        other => panic!("expected MissingScope, got {other:?}"),
    }
}

#[test]
fn list_ordered_descending_with_range() {
    let body = r#"{"orderedDataStoreEntries":[{"path":"p","value":15,"id":"p1"},{"path":"p","value":12,"id":"p2"}],"nextPageToken":"n"}"#;
    let (root, log) = serve(vec![(200, body)]);
    let query = OrderedQuery {
        descending: true,
        min: Some(10),
        max: Some(30),
    };
    let page = client(&root).list_ordered(&at(), query, Some("t")).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), OBASE);
    assert_eq!(c.api_key.as_deref(), Some("k"));
    let q = c.query();
    assert!(q.contains("maxPageSize=100"), "{q}");
    assert!(q.contains("pageToken=t"), "{q}");
    assert!(q.contains("orderBy=value desc"), "{q}");
    assert!(q.contains("filter=entry >= 10 && entry <= 30"), "{q}");
    assert_eq!(page.next_page_token.as_deref(), Some("n"));
    assert_eq!(
        page.items,
        vec![
            OrderedEntry {
                key: "p1".into(),
                value: 15
            },
            OrderedEntry {
                key: "p2".into(),
                value: 12
            }
        ]
    );
}

#[test]
fn list_ordered_only_min_and_only_max() {
    let (root, log) = serve(vec![(200, "{}"), (200, "{}")]);
    let c = client(&root);
    let min = OrderedQuery {
        min: Some(-5),
        ..OrderedQuery::default()
    };
    let max = OrderedQuery {
        max: Some(9),
        ..OrderedQuery::default()
    };
    let page = c.list_ordered(&at(), min, None).unwrap();
    assert!(page.items.is_empty());
    c.list_ordered(&at(), max, None).unwrap();
    let log = log.lock().unwrap();
    let q0 = log[0].query();
    let q1 = log[1].query();
    assert!(q0.contains("filter=entry >= -5"), "{q0}");
    assert!(!q0.contains("<="), "{q0}");
    assert!(q1.contains("filter=entry <= 9"), "{q1}");
    assert!(!q1.contains(">="), "{q1}");
    assert!(!q0.contains("orderBy") && !q1.contains("orderBy"));
}

#[test]
fn list_ordered_default_has_no_filter() {
    let (root, log) = serve(vec![(200, "{}")]);
    client(&root)
        .list_ordered(&at(), OrderedQuery::default(), None)
        .unwrap();
    let q = one(&log).query();
    assert!(!q.contains("filter") && !q.contains("orderBy") && !q.contains("pageToken"));
}

#[test]
fn get_ordered_parses_entry() {
    let (root, log) = serve(vec![(200, ORDERED)]);
    let e = client(&root).get_ordered(&at(), "p 1").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{OBASE}/p%201"));
    assert_eq!(
        e,
        OrderedEntry {
            key: "p1".into(),
            value: 15
        }
    );
}

#[test]
fn create_ordered_posts_value() {
    let (root, log) = serve(vec![(200, ORDERED)]);
    client(&root).create_ordered(&at(), "p1", 15).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), OBASE);
    assert!(c.query().contains("id=p1"), "{}", c.query());
    assert_eq!(c.json(), json!({"value": 15}));
}

#[test]
fn update_ordered_patches_value() {
    let (root, log) = serve(vec![(200, ORDERED)]);
    client(&root).update_ordered(&at(), "p1", -4).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "PATCH");
    assert_eq!(c.path(), format!("{OBASE}/p1"));
    assert_eq!(c.json(), json!({"value": -4}));
}

#[test]
fn increment_and_delete_ordered() {
    let (root, log) = serve(vec![(200, ORDERED), (200, "{}")]);
    let c = client(&root);
    let e = c.increment_ordered(&at(), "p1", 3).unwrap();
    c.delete_ordered(&at(), "p1").unwrap();
    assert_eq!(e.value, 15);
    let log = log.lock().unwrap();
    assert_eq!(log[0].method, "POST");
    assert_eq!(log[0].path(), format!("{OBASE}/p1:increment"));
    assert_eq!(log[0].json(), json!({"amount": 3}));
    assert_eq!(log[1].method, "DELETE");
    assert_eq!(log[1].path(), format!("{OBASE}/p1"));
}

#[test]
fn ordered_403_names_read_and_write_scopes() {
    let denied = r#"{"errors":[{"code":0,"message":"Scope not authorized."}]}"#;
    let (root, _log) = serve(vec![(403, denied), (403, denied), (403, denied)]);
    let c = client(&root);
    assert_eq!(
        missing_scope(c.get_ordered(&at(), "k").unwrap_err()),
        "universe.ordered-data-store.scope.entry:read"
    );
    assert_eq!(
        missing_scope(c.create_ordered(&at(), "k", 1).unwrap_err()),
        "universe.ordered-data-store.scope.entry:write"
    );
    assert_eq!(
        missing_scope(c.delete_ordered(&at(), "k").unwrap_err()),
        "universe.ordered-data-store.scope.entry:write"
    );
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
