//! Standard data store routes.

use serde_json::{json, Map, Value};

use super::fixture::{EntryId, Record, Rev, State, StoreMeta};
use super::{fail, listing, ok, page, starts_with, Kind, Reply, Request, Route};

const STALE_ETAG: &str =
    "Requested aborted because the provided etag does not match the latest version of the entry.";

pub(super) fn handle(st: &mut State, req: &Request, r: &Route) -> Option<Reply> {
    Some(match r.kind {
        Kind::ListStores => list_stores(st, req),
        Kind::ListEntries => list_entries(st, req, r),
        Kind::CreateEntry => create(st, req, r),
        Kind::GetEntry => get(st, req, r),
        Kind::UpdateEntry => update(st, req, r),
        Kind::Increment => increment(st, req, r),
        Kind::DeleteEntry => delete(st, r),
        Kind::ListRevisions => list_revisions(st, req, r),
        _ => return None,
    })
}

fn entry_path(u: &str, ds: &str, sc: &str, key: &str) -> String {
    format!("universes/{u}/data-stores/{ds}/scopes/{sc}/entries/{key}")
}

fn entry_json(u: &str, id: &EntryId, created: &str, rev: &Rev) -> Value {
    let (ds, sc, key) = id;
    let mut v = json!({
        "path": entry_path(u, ds, sc, key),
        "createTime": created,
        "revisionId": rev.id,
        "revisionCreateTime": rev.time,
        "state": "ACTIVE",
        "etag": rev.id,
        "value": rev.value,
        "id": key,
    });
    if !rev.users.is_empty() {
        v["users"] = json!(rev
            .users
            .iter()
            .map(|u| format!("users/{u}"))
            .collect::<Vec<_>>());
    }
    if !rev.attributes.is_empty() {
        v["attributes"] = Value::Object(rev.attributes.clone());
    }
    v
}

fn write_parts(body: &Value) -> (Value, Vec<u64>, Map<String, Value>) {
    let users = body
        .get("users")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|u| u.as_str()?.strip_prefix("users/")?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let attributes = body
        .get("attributes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    (
        body.get("value").cloned().unwrap_or(Value::Null),
        users,
        attributes,
    )
}

fn entry_id(r: &Route, key: &str) -> EntryId {
    (r.store.clone(), r.scope.clone(), key.to_owned())
}

/// Appends a revision, creating the record on first write; every write is a
/// new revision whose id doubles as the etag.
fn store_rev(st: &mut State, u: &str, id: EntryId, rev: Rev) -> Value {
    let rec = st.entries.entry(id.clone()).or_insert_with(|| Record {
        created: rev.time.clone(),
        revs: Vec::new(),
    });
    let json = entry_json(u, &id, &rec.created, &rev);
    rec.revs.push(rev);
    json
}

fn store_json(u: &str, s: &StoreMeta) -> Value {
    let mut v = json!({
        "path": format!("universes/{u}/data-stores/{}", s.name),
        "createTime": s.created,
        "state": if s.deleted { "DELETED" } else { "ACTIVE" },
        "id": s.name,
    });
    if let Some(expire) = &s.expire {
        v["expireTime"] = json!(expire);
    }
    v
}

fn list_stores(st: &State, req: &Request) -> Reply {
    let prefix = starts_with(req);
    let show_deleted = req.q("showDeleted") == Some("true");
    let all: Vec<Value> = st
        .stores
        .iter()
        .filter(|s| (show_deleted || !s.deleted) && s.name.starts_with(&prefix))
        .map(|s| store_json(&req.universe, s))
        .collect();
    let (items, next) = page(&all, req, 100);
    listing("dataStores", items, next)
}

fn list_entries(st: &State, req: &Request, r: &Route) -> Reply {
    let prefix = starts_with(req);
    let all: Vec<Value> = st
        .entries
        .keys()
        .filter(|(ds, sc, _)| *ds == r.store && (r.scope == "-" || *sc == r.scope))
        .filter_map(|(ds, sc, key)| {
            let id = if r.scope == "-" {
                format!("{sc}/{key}")
            } else {
                key.clone()
            };
            id.starts_with(&prefix)
                .then(|| json!({"path": entry_path(&req.universe, ds, sc, key), "id": id}))
        })
        .collect();
    let (items, next) = page(&all, req, 256);
    listing("dataStoreEntries", items, next)
}

fn create(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(key) = req.q("id") else {
        return fail(400, 3, "Missing entry id.");
    };
    let id = entry_id(r, key);
    if r.scope == "-" || st.entries.contains_key(&id) {
        return fail(400, 3, "Entry already exists.");
    }
    let (value, users, attributes) = write_parts(&req.body);
    let rev = st.new_rev(value, users, attributes);
    ok(store_rev(st, &req.universe, id, rev))
}

/// Splits `key@revision`; a literal key containing `@` wins when it exists.
fn resolve(st: &State, r: &Route) -> (String, Option<String>) {
    let exists = |k: &str| st.entries.contains_key(&entry_id(r, k));
    if exists(&r.key) {
        return (r.key.clone(), None);
    }
    match r.key.rsplit_once('@') {
        Some((key, rev)) if exists(key) => (key.to_owned(), Some(rev.to_owned())),
        _ => (r.key.clone(), None),
    }
}

fn get(st: &State, req: &Request, r: &Route) -> Reply {
    let (key, revision) = resolve(st, r);
    let id = entry_id(r, &key);
    let Some(rec) = st.entries.get(&id) else {
        return fail(404, 5, "Entry not found.");
    };
    let rev = match revision.as_deref() {
        None | Some("latest") => rec.revs.last(),
        Some(wanted) => rec.revs.iter().find(|v| v.id == wanted),
    };
    match rev {
        Some(rev) => ok(entry_json(&req.universe, &id, &rec.created, rev)),
        None => fail(404, 5, "Entry not found at revision."),
    }
}

fn update(st: &mut State, req: &Request, r: &Route) -> Reply {
    let id = entry_id(r, &r.key);
    let Some(rec) = st.entries.get(&id) else {
        return fail(404, 5, "Entry not found.");
    };
    let current = rec.revs.last().map(|v| v.id.as_str());
    if let Some(sent) = req.body.get("etag").and_then(Value::as_str) {
        if Some(sent) != current {
            return fail(409, 10, STALE_ETAG);
        }
    }
    let (value, users, attributes) = write_parts(&req.body);
    let rev = st.new_rev(value, users, attributes);
    ok(store_rev(st, &req.universe, id, rev))
}

fn increment(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(amount) = req.body.get("amount").and_then(Value::as_i64) else {
        return fail(400, 3, "The amount must be an integer.");
    };
    let id = entry_id(r, &r.key);
    let existing = st
        .entries
        .get(&id)
        .and_then(|rec| rec.revs.last())
        .map(|v| v.value.clone());
    let value = match existing {
        None => json!(amount),
        Some(v) => match (v.as_i64(), v.as_f64()) {
            (Some(n), _) => json!(n.saturating_add(amount)),
            (None, Some(f)) => json!(f + amount as f64),
            _ => return fail(400, 3, "The existing value is not numeric."),
        },
    };
    let (_, users, attributes) = write_parts(&req.body);
    let rev = st.new_rev(value, users, attributes);
    ok(store_rev(st, &req.universe, id, rev))
}

fn delete(st: &mut State, r: &Route) -> Reply {
    if st.entries.remove(&entry_id(r, &r.key)).is_some() {
        ok(json!({}))
    } else {
        fail(404, 5, "Entry not found.")
    }
}

fn list_revisions(st: &State, req: &Request, r: &Route) -> Reply {
    let id = entry_id(r, &r.key);
    let Some(rec) = st.entries.get(&id) else {
        return fail(404, 5, "Entry not found.");
    };
    let all: Vec<Value> = rec
        .revs
        .iter()
        .rev()
        .map(|v| {
            json!({
                "path": format!("{}@{}", entry_path(&req.universe, &id.0, &id.1, &id.2), v.id),
                "id": format!("{}@{}", id.2, v.id),
                "createTime": rec.created,
                "revisionId": v.id,
                "revisionCreateTime": v.time,
                "state": "ACTIVE",
                "etag": v.id,
            })
        })
        .collect();
    let (items, next) = page(&all, req, 100);
    listing("dataStoreEntries", items, next)
}
