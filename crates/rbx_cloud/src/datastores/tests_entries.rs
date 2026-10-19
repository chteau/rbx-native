use super::tests::{at, client, one, serve, BASE, ENTRY};
use super::*;
use serde_json::json;

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
fn delete_entry_sends_delete() {
    let (root, log) = serve(vec![(200, "{}")]);
    client(&root).delete_entry(&at(), "a b").unwrap();
    let c = one(&log);
    assert_eq!(c.method, "DELETE");
    assert_eq!(c.path(), format!("{BASE}/a%20b"));
    assert_eq!(c.api_key.as_deref(), Some("k"));
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
