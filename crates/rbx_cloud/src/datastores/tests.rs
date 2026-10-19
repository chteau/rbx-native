use super::*;
use crate::{ApiKey, Client, CloudError};
use serde_json::{json, Map, Value};
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
        store: "Players".into(),
        scope: "global".into(),
    }
}

const BASE: &str = "/cloud/v2/universes/7/data-stores/Players/scopes/global/entries";

const ENTRY: &str = r#"{"path":"universes/7/data-stores/Players/scopes/global/entries/k","createTime":"2026-01-01T00:00:00Z","revisionId":"REV1","revisionCreateTime":"2026-01-02T00:00:00Z","state":"ACTIVE","etag":"E1","value":{"coins":5},"id":"k","users":["users/925308243"],"attributes":{"a":"b"}}"#;

fn one(log: &Log) -> Captured {
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1, "{log:?}");
    log[0].clone()
}

#[test]
fn list_data_stores_sends_query_and_parses() {
    let body = r#"{"dataStores":[{"path":"universes/7/data-stores/A","createTime":"t","state":"ACTIVE","id":"A"},{"path":"p","state":"DELETED","expireTime":"x","id":"B"}],"nextPageToken":"n"}"#;
    let (root, log) = serve(vec![(200, body)]);
    let page = client(&root)
        .list_data_stores(7, "pre", true, Some("tok"))
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), "/cloud/v2/universes/7/data-stores");
    assert_eq!(c.api_key.as_deref(), Some("k"));
    let q = c.query();
    assert!(q.contains("maxPageSize=100"), "{q}");
    assert!(q.contains("pageToken=tok"), "{q}");
    assert!(q.contains("filter=id.startsWith(\"pre\")"), "{q}");
    assert!(q.contains("showDeleted=true"), "{q}");
    assert_eq!(page.next_page_token.as_deref(), Some("n"));
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].id, "A");
    assert_eq!(page.items[0].create_time.as_deref(), Some("t"));
    assert!(!page.items[0].deleted);
    assert!(page.items[1].deleted);
    assert_eq!(page.items[1].expire_time.as_deref(), Some("x"));
}

#[test]
fn list_data_stores_empty_object_is_empty_page() {
    let (root, log) = serve(vec![(200, "{}")]);
    let page = client(&root).list_data_stores(7, "", false, None).unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.next_page_token, None);
    let q = one(&log).query();
    assert!(!q.contains("filter"), "{q}");
    assert!(!q.contains("pageToken"), "{q}");
    assert!(!q.contains("showDeleted"), "{q}");
}

#[test]
fn list_entries_in_one_scope() {
    let body = r#"{"entries":[{"path":"universes/7/data-stores/Players/scopes/global/entries/k","id":"k"}],"dataStoreEntries":[{"path":"universes/7/data-stores/Players/scopes/global/entries/k","id":"k"}]}"#;
    let (root, log) = serve(vec![(200, body)]);
    let page = client(&root).list_entries(&at(), "pre", None).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), BASE);
    let q = c.query();
    assert!(q.contains("maxPageSize=256"), "{q}");
    assert!(q.contains("filter=id.startsWith(\"pre\")"), "{q}");
    assert_eq!(
        page.items,
        vec![EntryRef {
            scope: "global".into(),
            key: "k".into()
        }]
    );
}

#[test]
fn list_entries_wildcard_scope_splits_id() {
    let body = r#"{"dataStoreEntries":[{"path":"universes/7/data-stores/Players/scopes/global/entries/k","id":"global/k"},{"path":"universes/7/data-stores/Players/scopes/other/entries/x","id":"other/x"}],"nextPageToken":"more"}"#;
    let (root, log) = serve(vec![(200, body)]);
    let mut wild = at();
    wild.scope = ALL_SCOPES.into();
    let page = client(&root).list_entries(&wild, "", Some("t")).unwrap();
    let c = one(&log);
    assert_eq!(
        c.path(),
        "/cloud/v2/universes/7/data-stores/Players/scopes/-/entries"
    );
    assert!(c.query().contains("pageToken=t"));
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].scope, "global");
    assert_eq!(page.items[0].key, "k");
    assert_eq!(page.items[1].scope, "other");
    assert_eq!(page.items[1].key, "x");
    assert_eq!(page.next_page_token.as_deref(), Some("more"));
}

#[test]
fn list_entries_empty_page() {
    let (root, _log) = serve(vec![(200, "{}")]);
    let page = client(&root).list_entries(&at(), "", None).unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.next_page_token, None);
}

#[test]
fn get_entry_parses_every_field() {
    let (root, log) = serve(vec![(200, ENTRY)]);
    let e = client(&root).get_entry(&at(), "k").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{BASE}/k"));
    assert_eq!(c.api_key.as_deref(), Some("k"));
    assert_eq!(e.scope, "global");
    assert_eq!(e.key, "k");
    assert_eq!(e.value, json!({"coins": 5}));
    assert_eq!(e.etag, "E1");
    assert_eq!(e.revision_id, "REV1");
    assert_eq!(e.create_time.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(
        e.revision_create_time.as_deref(),
        Some("2026-01-02T00:00:00Z")
    );
    assert!(!e.deleted);
    assert_eq!(e.users, vec![925308243]);
    assert_eq!(e.attributes.get("a"), Some(&json!("b")));
}

#[test]
fn get_entry_without_users_or_attributes() {
    let body = r#"{"path":"universes/7/data-stores/Players/scopes/global/entries/k","revisionId":"R","state":"DELETED","etag":"E","value":3,"id":"k"}"#;
    let (root, _log) = serve(vec![(200, body)]);
    let e = client(&root).get_entry(&at(), "k").unwrap();
    assert!(e.users.is_empty());
    assert!(e.attributes.is_empty());
    assert!(e.deleted);
    assert_eq!(e.value, json!(3));
}

#[test]
fn get_entry_encodes_key_and_appends_latest_for_at_sign() {
    let (root, log) = serve(vec![(200, ENTRY)]);
    client(&root).get_entry(&at(), "a b@c").unwrap();
    let c = one(&log);
    assert_eq!(c.path(), format!("{BASE}/a%20b%40c@latest"));
}

#[test]
fn get_entry_revision_uses_at_revision() {
    let (root, log) = serve(vec![(200, ENTRY)]);
    client(&root)
        .get_entry_revision(&at(), "a b", "REV9")
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{BASE}/a%20b@REV9"));
}

#[test]
fn list_revisions_parses_page() {
    let body = r#"{"dataStoreEntries":[{"path":"p","id":"k@R2","createTime":"c","revisionId":"R2","revisionCreateTime":"t2","state":"ACTIVE","etag":"e2"},{"path":"p","id":"k@R1","revisionId":"R1","state":"DELETED","etag":"e1"}],"nextPageToken":"nx"}"#;
    let (root, log) = serve(vec![(200, body)]);
    let page = client(&root)
        .list_revisions(&at(), "k", Some("pt"))
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "GET");
    assert_eq!(c.path(), format!("{BASE}/k:listRevisions"));
    let q = c.query();
    assert!(q.contains("maxPageSize=100"), "{q}");
    assert!(q.contains("pageToken=pt"), "{q}");
    assert_eq!(page.next_page_token.as_deref(), Some("nx"));
    assert_eq!(
        page.items[0],
        Revision {
            revision_id: "R2".into(),
            revision_create_time: Some("t2".into()),
            etag: "e2".into(),
            deleted: false
        }
    );
    assert!(page.items[1].deleted);
    assert_eq!(page.items[1].revision_create_time, None);
}

#[test]
fn list_revisions_empty() {
    let (root, _log) = serve(vec![(200, "{}")]);
    let page = client(&root).list_revisions(&at(), "k", None).unwrap();
    assert!(page.items.is_empty());
}

#[test]
fn create_entry_posts_id_and_body() {
    let (root, log) = serve(vec![(200, ENTRY)]);
    let mut attrs = Map::new();
    attrs.insert("x".into(), json!("y"));
    let write = EntryWrite {
        value: json!({"a": 1}),
        users: vec![1, 2],
        attributes: attrs,
        etag: Some("ignored-or-not".into()),
    };
    client(&root).create_entry(&at(), "a b", &write).unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), BASE);
    assert_eq!(c.api_key.as_deref(), Some("k"));
    assert!(c.query().contains("id=a b"), "{}", c.query());
    let b = c.json();
    assert_eq!(b["value"], json!({"a": 1}));
    assert_eq!(b["users"], json!(["users/1", "users/2"]));
    assert_eq!(b["attributes"], json!({"x": "y"}));
}

#[test]
fn update_entry_sends_etag_only_when_some() {
    let (root, log) = serve(vec![(200, ENTRY), (200, ENTRY)]);
    let mut write = EntryWrite {
        value: json!(1),
        users: vec![5],
        ..EntryWrite::default()
    };
    let c = client(&root);
    c.update_entry(&at(), "k", &write).unwrap();
    write.etag = Some("E1".into());
    c.update_entry(&at(), "k", &write).unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log[0].method, "PATCH");
    assert_eq!(log[0].path(), format!("{BASE}/k"));
    assert!(log[0].json().get("etag").is_none());
    assert_eq!(log[0].json()["users"], json!(["users/5"]));
    assert_eq!(log[1].json()["etag"], json!("E1"));
    assert_eq!(log[1].json()["value"], json!(1));
}

#[test]
fn increment_entry_posts_amount() {
    let (root, log) = serve(vec![(200, ENTRY)]);
    let mut attrs = Map::new();
    attrs.insert("q".into(), json!(1));
    client(&root)
        .increment_entry(&at(), "a@b", -3, &[9], &attrs)
        .unwrap();
    let c = one(&log);
    assert_eq!(c.method, "POST");
    assert_eq!(c.path(), format!("{BASE}/a%40b:increment"));
    let b = c.json();
    assert_eq!(b["amount"], json!(-3));
    assert_eq!(b["users"], json!(["users/9"]));
    assert_eq!(b["attributes"], json!({"q": 1}));
}

#[test]
fn increment_entry_non_numeric_is_refused() {
    let (root, _log) = serve(vec![(
        400,
        r#"{"code":3,"message":"The existing value is not numeric."}"#,
    )]);
    let err = client(&root)
        .increment_entry(&at(), "k", 1, &[], &Map::new())
        .unwrap_err();
    assert!(matches!(
        err,
        CloudError::Refused { status: 400, ref message } if message == "The existing value is not numeric."
    ));
}

#[test]
fn delete_entry_sends_delete() {
    let (root, log) = serve(vec![(200, "{}")]);
    client(&root).delete_entry(&at(), "a b").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "DELETE");
    assert_eq!(c.path(), format!("{BASE}/a%20b"));
    assert_eq!(c.api_key.as_deref(), Some("k"));
}

#[test]
fn missing_entry_is_refused_with_message() {
    let (root, _log) = serve(vec![(404, r#"{"code":5,"message":"Entry not found."}"#)]);
    let err = client(&root).get_entry(&at(), "k").unwrap_err();
    assert!(matches!(
        err,
        CloudError::Refused { status: 404, ref message } if message == "Entry not found."
    ));
}

#[test]
fn gateway_403_is_missing_scope() {
    let (root, _log) = serve(vec![(
        403,
        r#"{"errors":[{"code":0,"message":"Scope not authorized."}]}"#,
    )]);
    let err = client(&root).get_entry(&at(), "k").unwrap_err();
    match err {
        CloudError::MissingScope { scope, message } => {
            assert_eq!(scope, "universe-datastores.objects:read");
            assert!(message.contains("Scope not authorized"), "{message}");
        }
        other => panic!("expected MissingScope, got {other:?}"),
    }
}

#[test]
fn delete_403_names_delete_scope() {
    let (root, _log) = serve(vec![(403, r#"{"code":7,"message":"nope"}"#)]);
    let err = client(&root).delete_entry(&at(), "k").unwrap_err();
    assert!(matches!(
        err,
        CloudError::MissingScope { ref scope, .. } if scope == "universe-datastores.objects:delete"
    ));
}

#[test]
fn stale_etag_409_is_conflict() {
    let (root, _log) = serve(vec![(
        409,
        r#"{"code":10,"message":"Requested aborted because the provided etag does not match the latest version of the entry."}"#,
    )]);
    let write = EntryWrite {
        value: json!(1),
        etag: Some("old".into()),
        ..EntryWrite::default()
    };
    let err = client(&root).update_entry(&at(), "k", &write).unwrap_err();
    match err {
        CloudError::Conflict { message } => assert!(message.contains("etag"), "{message}"),
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn get_retries_after_two_throttles() {
    let (root, log) = serve(vec![(429, "{}"), (429, "{}"), (200, ENTRY)]);
    let e = client(&root).get_entry(&at(), "k").unwrap();
    assert_eq!(e.etag, "E1");
    assert_eq!(log.lock().unwrap().len(), 3);
}

#[test]
fn patch_retries_after_two_throttles() {
    let (root, log) = serve(vec![(429, "{}"), (429, "{}"), (200, ENTRY)]);
    let write = EntryWrite {
        value: json!(1),
        ..EntryWrite::default()
    };
    let e = client(&root).update_entry(&at(), "k", &write).unwrap();
    assert_eq!(e.revision_id, "REV1");
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 3);
    assert!(log.iter().all(|c| c.method == "PATCH"));
}

#[test]
fn rollback_reads_revision_then_patches_with_current_etag() {
    let old = r#"{"path":"p","revisionId":"R0","state":"ACTIVE","etag":"OLD","value":{"v":0},"id":"k","users":["users/4"],"attributes":{"z":"1"}}"#;
    let (root, log) = serve(vec![(200, old), (200, ENTRY)]);
    client(&root)
        .rollback_entry(&at(), "k", "R0", "CURRENT")
        .unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].method, "GET");
    assert_eq!(log[0].path(), format!("{BASE}/k@R0"));
    assert_eq!(log[1].method, "PATCH");
    assert_eq!(log[1].path(), format!("{BASE}/k"));
    let b = log[1].json();
    assert_eq!(b["value"], json!({"v": 0}));
    assert_eq!(b["users"], json!(["users/4"]));
    assert_eq!(b["attributes"], json!({"z": "1"}));
    assert_eq!(b["etag"], json!("CURRENT"));
}

#[test]
fn rollback_stops_when_revision_is_missing() {
    let (root, log) = serve(vec![(
        404,
        r#"{"code":5,"message":"Entry not found at revision."}"#,
    )]);
    let err = client(&root)
        .rollback_entry(&at(), "k", "R0", "E")
        .unwrap_err();
    assert!(matches!(err, CloudError::Refused { status: 404, .. }));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn validate_entry_accepts_reasonable_write() {
    let write = EntryWrite {
        value: json!({"a": 1}),
        ..EntryWrite::default()
    };
    assert!(validate_entry("key", &write).is_ok());
    assert!(validate_entry(&"k".repeat(MAX_NAME_CHARS), &write).is_ok());
}

#[test]
fn validate_entry_rejects_bad_keys() {
    let write = EntryWrite::default();
    assert!(validate_entry("", &write).is_err());
    assert!(validate_entry(&"k".repeat(MAX_NAME_CHARS + 1), &write).is_err());
}

#[test]
fn validate_entry_rejects_oversized_value_and_attributes() {
    let big = EntryWrite {
        value: Value::String("x".repeat(MAX_VALUE_BYTES)),
        ..EntryWrite::default()
    };
    assert!(validate_entry("k", &big).is_err());
    let mut attributes = Map::new();
    attributes.insert("a".into(), Value::String("y".repeat(MAX_ATTRIBUTES_CHARS)));
    let long = EntryWrite {
        value: json!(1),
        attributes,
        ..EntryWrite::default()
    };
    assert!(validate_entry("k", &long).is_err());
}

#[test]
fn value_bytes_is_serialized_length() {
    assert_eq!(value_bytes(&json!(null)), 4);
    assert_eq!(value_bytes(&json!({"a": [1, 2]})), 13);
    assert_eq!(value_bytes(&json!("é")), 4);
}

#[test]
fn segment_percent_encodes_all_but_unreserved() {
    assert_eq!(segment("AZaz09-._~"), "AZaz09-._~");
    assert_eq!(segment("a b@c/d"), "a%20b%40c%2Fd");
    assert_eq!(segment("é"), "%C3%A9");
    assert_eq!(segment(""), "");
}
