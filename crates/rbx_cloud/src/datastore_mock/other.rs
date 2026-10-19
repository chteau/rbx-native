//! Ordered data store and memory store (sorted map, queue) routes.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::fixture::{iso, MapItem, QueueItem, Sort, State};
use super::{conditions, fail, holds, listing, ok, page, Kind, Reply, Request, Route};

const STALE_MAP_ETAG: &str = "The provided etag does not match the latest version of the item.";

pub(super) fn handle(st: &mut State, req: &Request, r: &Route) -> Option<Reply> {
    Some(match r.kind {
        Kind::OrderedList => ordered_list(st, req, r),
        Kind::OrderedGet => ordered_get(st, req, r),
        Kind::OrderedCreate => ordered_create(st, req, r),
        Kind::OrderedUpdate => ordered_update(st, req, r),
        Kind::OrderedIncrement => ordered_increment(st, req, r),
        Kind::OrderedDelete => ordered_delete(st, r),
        Kind::MapList => map_list(st, req, r),
        Kind::MapGet => map_get(st, req, r),
        Kind::MapCreate => map_create(st, req, r),
        Kind::MapUpdate => map_update(st, req, r),
        Kind::MapDelete => map_delete(st, r),
        Kind::QueueAdd => queue_add(st, req, r),
        Kind::QueueRead => queue_read(st, req, r),
        Kind::QueueDiscard => queue_discard(st, req, r),
        _ => return None,
    })
}

fn ordered_json(u: &str, r: &Route, key: &str, value: i64) -> Value {
    json!({
        "path": format!(
            "universes/{u}/ordered-data-stores/{}/scopes/{}/entries/{key}",
            r.store, r.scope
        ),
        "value": value,
        "id": key,
    })
}

fn ordered_rows<'a>(
    st: &'a mut State,
    r: &Route,
) -> &'a mut std::collections::BTreeMap<String, i64> {
    st.ordered
        .entry((r.store.clone(), r.scope.clone()))
        .or_default()
}

fn ordered_list(st: &mut State, req: &Request, r: &Route) -> Reply {
    let descending = req
        .q("orderBy")
        .is_some_and(|o| o.trim_end().ends_with("desc"));
    let conds = conditions(req);
    let mut rows: Vec<(String, i64)> = ordered_rows(st, r)
        .iter()
        .filter(|(_, v)| {
            conds
                .iter()
                .all(|(op, operand)| operand.parse::<i64>().map_or(true, |n| holds(**v, op, n)))
        })
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    if descending {
        rows.reverse();
    }
    let all: Vec<Value> = rows
        .iter()
        .map(|(k, v)| ordered_json(&req.universe, r, k, *v))
        .collect();
    let (items, next) = page(&all, req, 100);
    listing("orderedDataStoreEntries", items, next)
}

fn ordered_get(st: &mut State, req: &Request, r: &Route) -> Reply {
    match ordered_rows(st, r).get(&r.key) {
        Some(v) => ok(ordered_json(&req.universe, r, &r.key, *v)),
        None => fail(404, 5, "Entry not found."),
    }
}

fn ordered_create(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(key) = req.q("id") else {
        return fail(400, 3, "Missing entry id.");
    };
    let Some(value) = req.body.get("value").and_then(Value::as_i64) else {
        return fail(400, 3, "The value must be an integer.");
    };
    let rows = ordered_rows(st, r);
    if rows.contains_key(key) {
        return fail(400, 3, "Entry already exists.");
    }
    rows.insert(key.to_owned(), value);
    ok(ordered_json(&req.universe, r, key, value))
}

fn ordered_update(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(value) = req.body.get("value").and_then(Value::as_i64) else {
        return fail(400, 3, "The value must be an integer.");
    };
    match ordered_rows(st, r).get_mut(&r.key) {
        Some(slot) => {
            *slot = value;
            ok(ordered_json(&req.universe, r, &r.key, value))
        }
        None => fail(404, 5, "Entry not found."),
    }
}

fn ordered_increment(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(amount) = req.body.get("amount").and_then(Value::as_i64) else {
        return fail(400, 3, "The amount must be an integer.");
    };
    let slot = ordered_rows(st, r).entry(r.key.clone()).or_insert(0);
    *slot = slot.saturating_add(amount);
    ok(ordered_json(&req.universe, r, &r.key, *slot))
}

fn ordered_delete(st: &mut State, r: &Route) -> Reply {
    if ordered_rows(st, r).remove(&r.key).is_some() {
        ok(json!({}))
    } else {
        fail(404, 5, "Entry not found.")
    }
}

fn item_json(u: &str, map: &str, key: &str, item: &MapItem) -> Value {
    let mut v = json!({
        "path": format!("universes/{u}/memory-store/sorted-maps/{map}/items/{key}"),
        "value": item.value,
        "etag": item.etag,
        "ttl": format!("{}s", item.ttl),
        "expireTime": iso(item.expire_at),
        "id": key,
    });
    match &item.sort {
        Some(Sort::Text(s)) => v["stringSortKey"] = json!(s),
        Some(Sort::Num(n)) => v["numericSortKey"] = json!(n),
        None => {}
    }
    v
}

fn map_list(st: &State, req: &Request, r: &Route) -> Reply {
    let descending = req
        .q("orderBy")
        .is_some_and(|o| o.trim_end().ends_with("desc"));
    let conds = conditions(req);
    let mut keys: Vec<&String> = st
        .maps
        .get(&r.store)
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
        .filter(|k| {
            conds
                .iter()
                .all(|(op, operand)| holds(k.as_str(), op, operand.as_str()))
        })
        .collect();
    if descending {
        keys.reverse();
    }
    let all: Vec<Value> = keys
        .iter()
        .filter_map(|k| {
            let item = st.maps.get(&r.store)?.get(*k)?;
            Some(item_json(&req.universe, &r.store, k, item))
        })
        .collect();
    let (items, next) = page(&all, req, 100);
    listing("items", items, next)
}

fn map_get(st: &State, req: &Request, r: &Route) -> Reply {
    match st.maps.get(&r.store).and_then(|m| m.get(&r.key)) {
        Some(item) => ok(item_json(&req.universe, &r.store, &r.key, item)),
        None => fail(404, 5, "Item not found."),
    }
}

struct ItemWrite {
    value: Value,
    ttl: u64,
    sort: Option<Sort>,
    etag: Option<String>,
}

fn parse_seconds(text: &str) -> Option<u64> {
    text.strip_suffix('s')?.parse().ok()
}

fn item_write(body: &Value) -> Result<ItemWrite, Reply> {
    let ttl = match body.get("ttl").and_then(Value::as_str) {
        None => 3600,
        Some(text) => match parse_seconds(text) {
            Some(t) if t <= 3_888_000 => t,
            _ => return Err(fail(400, 3, "The ttl must be at most 3888000s.")),
        },
    };
    let sort = if let Some(s) = body.get("stringSortKey").and_then(Value::as_str) {
        Some(Sort::Text(s.to_owned()))
    } else {
        body.get("numericSortKey")
            .and_then(Value::as_f64)
            .map(Sort::Num)
    };
    Ok(ItemWrite {
        value: body.get("value").cloned().unwrap_or(Value::Null),
        ttl,
        sort,
        etag: body.get("etag").and_then(Value::as_str).map(str::to_owned),
    })
}

fn map_create(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(key) = req.q("id") else {
        return fail(400, 3, "Missing item id.");
    };
    let write = match item_write(&req.body) {
        Ok(w) => w,
        Err(reply) => return reply,
    };
    if st.maps.get(&r.store).is_some_and(|m| m.contains_key(key)) {
        return fail(400, 3, "Item already exists.");
    }
    let item = MapItem {
        value: write.value,
        etag: st.new_id(),
        ttl: write.ttl,
        expire_at: st.tick() + write.ttl as i64,
        sort: write.sort,
    };
    let json = item_json(&req.universe, &r.store, key, &item);
    st.maps
        .entry(r.store.clone())
        .or_default()
        .insert(key.to_owned(), item);
    ok(json)
}

fn map_update(st: &mut State, req: &Request, r: &Route) -> Reply {
    let write = match item_write(&req.body) {
        Ok(w) => w,
        Err(reply) => return reply,
    };
    let Some(current) = st.maps.get(&r.store).and_then(|m| m.get(&r.key)) else {
        return fail(404, 5, "Item not found.");
    };
    if write
        .etag
        .as_deref()
        .is_some_and(|sent| sent != current.etag)
    {
        return fail(409, 10, STALE_MAP_ETAG);
    }
    let item = MapItem {
        value: write.value,
        etag: st.new_id(),
        ttl: write.ttl,
        expire_at: st.tick() + write.ttl as i64,
        sort: write.sort,
    };
    let json = item_json(&req.universe, &r.store, &r.key, &item);
    st.maps
        .entry(r.store.clone())
        .or_default()
        .insert(r.key.clone(), item);
    ok(json)
}

fn map_delete(st: &mut State, r: &Route) -> Reply {
    if st
        .maps
        .get_mut(&r.store)
        .is_some_and(|m| m.remove(&r.key).is_some())
    {
        ok(json!({}))
    } else {
        fail(404, 5, "Item not found.")
    }
}

fn queue_item_json(u: &str, queue: &str, item: &QueueItem) -> Value {
    json!({
        "path": format!("universes/{u}/memory-store/queues/{queue}/items/{}", item.id),
        "data": item.data,
        "priority": item.priority,
        "ttl": format!("{}s", item.ttl),
        "expireTime": iso(item.expire_at),
        "id": item.id,
    })
}

fn queue_add(st: &mut State, req: &Request, r: &Route) -> Reply {
    let ttl = match req.body.get("ttl").and_then(Value::as_str) {
        None => 3600,
        Some(text) => match parse_seconds(text) {
            Some(t) if t <= 3_888_000 => t,
            _ => return fail(400, 3, "The ttl must be at most 3888000s."),
        },
    };
    let item = QueueItem {
        id: st.new_id(),
        data: req.body.get("data").cloned().unwrap_or(Value::Null),
        priority: req
            .body
            .get("priority")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        ttl,
        expire_at: st.tick() + ttl as i64,
        hidden_until: None,
        read_id: None,
    };
    let json = queue_item_json(&req.universe, &r.store, &item);
    st.queues.entry(r.store.clone()).or_default().push(item);
    ok(json)
}

/// Highest priority first, then oldest; read items stay hidden for the window.
fn queue_read(st: &mut State, req: &Request, r: &Route) -> Reply {
    let count = req
        .q("count")
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 200);
    let window = req
        .q("invisibilityWindow")
        .and_then(parse_seconds)
        .unwrap_or(30);
    let now = Instant::now();
    let read_id = st.new_id();
    let Some(queue) = st.queues.get_mut(&r.store) else {
        return ok(json!({}));
    };
    let mut visible: Vec<usize> = (0..queue.len())
        .filter(|&i| queue[i].hidden_until.is_none_or(|t| t <= now))
        .collect();
    visible.sort_by(|&a, &b| {
        queue[b]
            .priority
            .total_cmp(&queue[a].priority)
            .then(a.cmp(&b))
    });
    visible.truncate(count);
    if visible.is_empty() {
        return ok(json!({}));
    }
    let items: Vec<Value> = visible
        .iter()
        .map(|&i| {
            let item = &mut queue[i];
            item.hidden_until = Some(now + Duration::from_secs(window));
            item.read_id = Some(read_id.clone());
            queue_item_json(&req.universe, &r.store, item)
        })
        .collect();
    ok(json!({"readId": read_id, "items": items}))
}

fn queue_discard(st: &mut State, req: &Request, r: &Route) -> Reply {
    let Some(read_id) = req.body.get("readId").and_then(Value::as_str) else {
        return fail(400, 3, "Missing readId.");
    };
    if let Some(queue) = st.queues.get_mut(&r.store) {
        queue.retain(|i| i.read_id.as_deref() != Some(read_id));
    }
    ok(json!({}))
}
