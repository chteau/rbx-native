use super::tests::{client, missing_scope, one, serve};
use super::*;
use serde_json::json;

fn at() -> StoreRef {
    StoreRef {
        universe_id: 7,
        store: "Top".into(),
        scope: "global".into(),
    }
}

const OBASE: &str = "/cloud/v2/universes/7/ordered-data-stores/Top/scopes/global/entries";
const ORDERED: &str = r#"{"path":"p","value":15,"id":"p1"}"#;

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
