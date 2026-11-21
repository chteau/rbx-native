//! A Local Run's DataStores as Studio's Play has them: the experience's
//! real ones, through Open Cloud with the editor's API key, for a published
//! place run online with API access on. Each call answers in the shape the
//! local store answers (see `rbx_lua::server::LiveData`), so the Luau side's
//! rules, budgets and messages are the same either way.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rbx_cloud::{Client, CloudError, EntryWrite, OrderedQuery, StoreRef, ALL_SCOPES};
use serde_json::{json, Map, Value};

/// The local store's bucket separator (`d`/`o`, name, scope).
const SEP: char = '\u{1}';

/// The store a connection check reads a missing key from.
const PROBE_STORE: &str = "RbxNativeLocalRunProbe";

pub struct LiveStores {
    client: Client,
    universe: u64,
    /// Writes fail instead of reaching the experience (a test guard).
    read_only: bool,
}

/// Why a run's DataStores stay local, one line for Output.
#[derive(Debug, Clone, PartialEq)]
pub enum Local {
    Offline,
    NoKey,
    /// The key can't read this experience's DataStores: the message Studio's
    /// kind of error carries.
    Refused(String),
}

impl Local {
    pub fn reason(&self) -> String {
        match self {
            Local::Offline => "offline".into(),
            Local::NoKey => "no Open Cloud API key: add one in Settings \u{203a} Account \u{203a} API key (scopes universe-datastores.objects:read/create/update/delete/list and universe-datastores.versions:list for this experience)".into(),
            Local::Refused(message) => message.clone(),
        }
    }
}

/// The experience `universe`'s DataStores, when they can be reached: online,
/// with a key that may read them (checked by reading a key that isn't
/// there). `read_only` turns every write into an error.
pub fn connect(universe: u64, read_only: bool) -> Result<Arc<LiveStores>, Local> {
    if rbx_net::is_offline() {
        return Err(Local::Offline);
    }
    let key = rbx_cloud::ApiKey::from_env_or_config()
        .or_else(rbx_cloud::keyring::stored_api_key)
        .ok_or(Local::NoKey)?;
    let live = LiveStores {
        client: Client::new(Some(key)),
        universe,
        read_only,
    };
    match live
        .client
        .get_entry(&live.at(PROBE_STORE, "global"), "probe")
    {
        Ok(_) => {}
        Err(err) if not_found(&err) => {}
        Err(CloudError::Transport(_)) => return Err(Local::Offline),
        Err(err) => return Err(Local::Refused(roblox_error(&err))),
    }
    Ok(Arc::new(live))
}

impl std::fmt::Debug for LiveStores {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LiveStores({}, read_only: {})",
            self.universe, self.read_only
        )
    }
}

impl LiveStores {
    fn at(&self, store: &str, scope: &str) -> StoreRef {
        StoreRef {
            universe_id: self.universe,
            store: store.to_owned(),
            scope: scope.to_owned(),
        }
    }

    /// `(ordered, where)` for a local bucket name.
    fn bucket(&self, bucket: &str) -> Result<(bool, StoreRef), String> {
        let mut parts = bucket.split(SEP);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(kind), Some(name), Some(scope)) => Ok((kind == "o", self.at(name, scope))),
            _ => Err(format!("bad bucket {bucket:?}")),
        }
    }

    fn write_guard(&self) -> Result<(), String> {
        match self.read_only {
            true => Err("Local Run's read-only guard is on: DataStore writes are refused".into()),
            false => Ok(()),
        }
    }

    fn get(&self, bucket: &str, key: &str) -> Result<Value, String> {
        let (ordered, at) = self.bucket(bucket)?;
        if ordered {
            return match self.client.get_ordered(&at, key) {
                Ok(found) => Ok(ordered_entry(found.value)),
                Err(err) if not_found(&err) => Ok(Value::Null),
                Err(err) => Err(roblox_error(&err)),
            };
        }
        match self.client.get_entry(&at, key) {
            Ok(found) if found.deleted => Ok(Value::Null),
            Ok(found) => Ok(entry(&found)),
            Err(err) if not_found(&err) => Ok(Value::Null),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    fn put(
        &self,
        bucket: &str,
        key: &str,
        value: &str,
        users: &str,
        meta: &str,
    ) -> Result<Value, String> {
        self.write_guard()?;
        let (ordered, at) = self.bucket(bucket)?;
        let parsed: Value = serde_json::from_str(value).map_err(|e| e.to_string())?;
        if ordered {
            let number = parsed.as_f64().unwrap_or(0.0).round() as i64;
            let written = match self.client.update_ordered(&at, key, number) {
                Err(err) if not_found(&err) => self.client.create_ordered(&at, key, number),
                other => other,
            };
            return written
                .map(|w| ordered_entry(w.value))
                .map_err(|e| roblox_error(&e));
        }
        let write = EntryWrite {
            value: parsed,
            users: serde_json::from_str(users).unwrap_or_default(),
            attributes: serde_json::from_str::<Map<String, Value>>(meta).unwrap_or_default(),
            etag: None,
        };
        let written = match self.client.update_entry(&at, key, &write) {
            Err(err) if not_found(&err) => self.client.create_entry(&at, key, &write),
            other => other,
        };
        written.map(|w| entry(&w)).map_err(|e| roblox_error(&e))
    }

    fn remove(&self, bucket: &str, key: &str) -> Result<Value, String> {
        self.write_guard()?;
        let old = self.get(bucket, key)?;
        if old.is_null() {
            return Ok(old);
        }
        let (ordered, at) = self.bucket(bucket)?;
        let gone = match ordered {
            true => self.client.delete_ordered(&at, key),
            false => self.client.delete_entry(&at, key),
        };
        match gone {
            Ok(()) => Ok(old),
            Err(err) if not_found(&err) => Ok(old),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    /// Every revision of `key`, oldest first.
    fn revisions(&self, bucket: &str, key: &str) -> Result<Vec<rbx_cloud::Revision>, String> {
        let (_, at) = self.bucket(bucket)?;
        let mut all = Vec::new();
        let mut token = None;
        loop {
            let page = match self.client.list_revisions(&at, key, token.as_deref()) {
                Ok(page) => page,
                Err(err) if not_found(&err) => break,
                Err(err) => return Err(roblox_error(&err)),
            };
            all.extend(page.items);
            token = page.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        all.sort_by_key(|r| millis(r.revision_create_time.as_deref()));
        Ok(all)
    }

    fn version(&self, bucket: &str, key: &str, id: &str) -> Result<Value, String> {
        let (_, at) = self.bucket(bucket)?;
        match self.client.get_entry_revision(&at, key, id) {
            Ok(found) => Ok(entry(&found)),
            Err(err) if not_found(&err) => Ok(Value::Null),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    fn keys(&self, name: &str, scope: Option<&str>, prefix: &str) -> Result<Value, String> {
        let at = self.at(name, scope.unwrap_or(ALL_SCOPES));
        let mut keys = Vec::new();
        let mut token = None;
        loop {
            let page = self
                .client
                .list_entries(&at, prefix, token.as_deref())
                .map_err(|e| roblox_error(&e))?;
            keys.extend(page.items.into_iter().map(|e| match scope {
                Some(_) => e.key,
                None => format!("{}/{}", e.scope, e.key),
            }));
            token = page.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        keys.sort();
        Ok(json!(keys))
    }

    fn stores(&self, prefix: &str) -> Result<Value, String> {
        let mut out = Vec::new();
        let mut token = None;
        loop {
            let page = self
                .client
                .list_data_stores(self.universe, prefix, false, token.as_deref())
                .map_err(|e| roblox_error(&e))?;
            for store in page.items {
                let created = millis(store.create_time.as_deref());
                out.push(json!({ "name": store.id, "created": created, "updated": created }));
            }
            token = page.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        Ok(Value::Array(out))
    }

    fn entries(&self, bucket: &str) -> Result<Value, String> {
        let (_, at) = self.bucket(bucket)?;
        let mut out = Vec::new();
        let mut token = None;
        let query = OrderedQuery {
            descending: false,
            min: None,
            max: None,
        };
        loop {
            let page = self
                .client
                .list_ordered(&at, query, token.as_deref())
                .map_err(|e| roblox_error(&e))?;
            for item in page.items {
                out.push(json!({ "key": item.key, "value": item.value.to_string() }));
            }
            token = page.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        Ok(Value::Array(out))
    }
}

impl rbx_lua::server::LiveData for LiveStores {
    fn call(&self, op: &str, args: &[Value]) -> Result<Value, String> {
        let text = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or_default();
        match op {
            "get" => self.get(text(0), text(1)),
            "put" => self.put(text(0), text(1), text(2), text(3), text(4)),
            "remove" => self.remove(text(0), text(1)),
            "versions" => Ok(Value::Array(
                self.revisions(text(0), text(1))?
                    .into_iter()
                    .map(|r| {
                        json!({
                            "version": r.revision_id,
                            "created": millis(r.revision_create_time.as_deref()),
                            "deleted": r.deleted,
                        })
                    })
                    .collect(),
            )),
            "version" => self.version(text(0), text(1), text(2)),
            "version_at" => {
                let time = args.get(2).and_then(Value::as_f64).unwrap_or(0.0) as i64;
                let before = self
                    .revisions(text(0), text(1))?
                    .into_iter()
                    .rfind(|r| millis(r.revision_create_time.as_deref()) <= time);
                match before {
                    Some(r) if !r.deleted => self.version(text(0), text(1), &r.revision_id),
                    _ => Ok(Value::Null),
                }
            }
            "remove_version" => {
                self.write_guard()?;
                Err("RemoveVersionAsync has no Open Cloud equivalent: versions of live DataStores can't be removed from Local Run".into())
            }
            "keys" => self.keys(text(0), args.get(1).and_then(Value::as_str), text(2)),
            "stores" => self.stores(text(0)),
            "entries" => self.entries(text(0)),
            other => Err(format!("unknown DataStore call {other}")),
        }
    }
}

/// A standard entry as the local store reports one.
fn entry(found: &rbx_cloud::Entry) -> Value {
    json!({
        "value": found.value.to_string(),
        "version": found.revision_id,
        "created": millis(found.create_time.as_deref()),
        "updated": millis(found.revision_create_time.as_deref()),
        "users": Value::from(found.users.clone()).to_string(),
        "meta": Value::Object(found.attributes.clone()).to_string(),
        "deleted": found.deleted,
    })
}

/// An ordered entry: a number, no versions or metadata.
fn ordered_entry(value: i64) -> Value {
    let now = now_ms();
    json!({
        "value": value.to_string(),
        "version": "",
        "created": now,
        "updated": now,
        "users": "[]",
        "meta": "{}",
        "deleted": false,
    })
}

fn not_found(err: &CloudError) -> bool {
    matches!(
        err,
        CloudError::Http { status: 404, .. } | CloudError::Refused { status: 404, .. }
    )
}

/// The message a script sees for a failed call, in the shape Studio's
/// DataStore errors take.
fn roblox_error(err: &CloudError) -> String {
    match err {
        CloudError::MissingScope { scope, message } => format!(
            "403: Forbidden: the Open Cloud API key lacks {scope} for this experience ({message})"
        ),
        CloudError::RateLimited { .. } => {
            "502: API Services rejected request with error. Error code: 0 Reason: rate limited by Open Cloud".into()
        }
        CloudError::Refused { status, message } => {
            format!("502: API Services rejected request with error. Error code: {status} Reason: {message}")
        }
        other => format!("502: API Services rejected request with error. Error code: 0 Reason: {other}"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Unix milliseconds of an RFC 3339 UTC time (`2024-05-01T12:30:00.123Z`),
/// 0 when absent or unreadable.
fn millis(time: Option<&str>) -> i64 {
    let Some(time) = time else { return 0 };
    let number = |range: std::ops::Range<usize>| time.get(range)?.parse::<i64>().ok();
    let (Some(y), Some(m), Some(d), Some(hh), Some(mm), Some(ss)) = (
        number(0..4),
        number(5..7),
        number(8..10),
        number(11..13),
        number(14..16),
        number(17..19),
    ) else {
        return 0;
    };
    let fraction = time
        .get(19..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .map_or(0, |digits| {
            format!("{digits:0<3}")[..3].parse::<i64>().unwrap_or(0)
        });
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_times_read_as_unix_milliseconds() {
        assert_eq!(millis(Some("1970-01-01T00:00:00Z")), 0);
        assert_eq!(millis(Some("2024-05-01T12:30:00.123Z")), 1_714_566_600_123);
        assert_eq!(
            millis(Some("2024-05-01T12:30:00.1234567Z")),
            1_714_566_600_123
        );
        assert_eq!(millis(None), 0);
    }

    #[test]
    fn buckets_name_their_store_and_scope() {
        let live = LiveStores {
            client: Client::new(None),
            universe: 7,
            read_only: true,
        };
        let (ordered, at) = live.bucket("o\u{1}Coins\u{1}global").unwrap();
        assert!(ordered);
        assert_eq!(
            (at.universe_id, at.store.as_str(), at.scope.as_str()),
            (7, "Coins", "global")
        );
        assert!(live
            .put("d\u{1}A\u{1}global", "k", "1", "[]", "{}")
            .is_err());
    }
}
