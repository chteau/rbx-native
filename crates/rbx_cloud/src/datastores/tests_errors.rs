use super::tests::{at, client, serve, ENTRY};
use super::*;
use crate::CloudError;
use serde_json::{json, Map};

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
