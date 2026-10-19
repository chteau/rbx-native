//! Open Cloud v2 data stores: standard entries here, ordered stores and memory
//! stores in their submodules.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::client::RawResponse;
use crate::error::{self, CloudError};

mod entries;
mod memory;
mod ordered;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_errors;
#[cfg(test)]
mod tests_memory;
#[cfg(test)]
mod tests_more;

pub use memory::*;
pub use ordered::*;

pub const MAX_VALUE_BYTES: usize = 4_194_304;
pub const MAX_NAME_CHARS: usize = 50;
pub const MAX_ATTRIBUTES_CHARS: usize = 300;
pub const MAX_ORDERED_KEY_CHARS: usize = 63;
pub const MAX_MEMORY_KEY_CHARS: usize = 127;
pub const MAX_MEMORY_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_MEMORY_TTL_SECONDS: u64 = 3_888_000;
/// Scope wildcard: list the entries of every scope at once.
pub const ALL_SCOPES: &str = "-";

#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRef {
    pub universe_id: u64,
    pub store: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataStoreInfo {
    pub id: String,
    pub create_time: Option<String>,
    pub deleted: bool,
    pub expire_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRef {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub scope: String,
    pub key: String,
    pub value: Value,
    pub etag: String,
    pub revision_id: String,
    pub create_time: Option<String>,
    pub revision_create_time: Option<String>,
    pub deleted: bool,
    pub users: Vec<u64>,
    pub attributes: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub revision_id: String,
    pub revision_create_time: Option<String>,
    pub etag: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntryWrite {
    pub value: Value,
    pub users: Vec<u64>,
    pub attributes: Map<String, Value>,
    pub etag: Option<String>,
}

/// Serialized size of a JSON value, which is what Roblox's value limit counts.
pub fn value_bytes(value: &Value) -> usize {
    use serde::Serialize;
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, SpacedFormatter);
    if value.serialize(&mut serializer).is_err() {
        return 0;
    }
    out.len()
}

/// Compact JSON with a space after every `:` and `,`, the form Roblox
/// measures its value limit against.
struct SpacedFormatter;

impl serde_json::ser::Formatter for SpacedFormatter {
    fn begin_array_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}

/// Checks a write against Roblox's documented limits before sending it.
pub fn validate_entry(key: &str, write: &EntryWrite) -> Result<(), String> {
    let chars = key.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Err(format!("A key needs 1 to {MAX_NAME_CHARS} characters."));
    }
    let bytes = value_bytes(&write.value);
    if bytes > MAX_VALUE_BYTES {
        return Err(format!(
            "The value is {bytes} bytes; the limit is {MAX_VALUE_BYTES}."
        ));
    }
    let attribute_chars = serde_json::to_string(&write.attributes).map_or(0, |s| s.chars().count());
    if attribute_chars > MAX_ATTRIBUTES_CHARS {
        return Err(format!(
            "The attributes are {attribute_chars} characters; the limit is {MAX_ATTRIBUTES_CHARS}."
        ));
    }
    Ok(())
}

/// Percent-encodes one URL path segment, keeping only RFC 3986 unreserved bytes.
pub(crate) fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Turns a response into JSON, naming `scope` when the key lacks it.
pub(crate) fn answer(url: &str, response: RawResponse, scope: &str) -> Result<Value, CloudError> {
    let said = || {
        serde_json::from_slice::<Value>(&response.body)
            .ok()
            .as_ref()
            .and_then(crate::assets::complaint)
    };
    match response.status {
        200..=299 => {
            if response.body.iter().all(u8::is_ascii_whitespace) {
                Ok(Value::Null)
            } else {
                Ok(serde_json::from_slice(&response.body)?)
            }
        }
        403 => Err(CloudError::MissingScope {
            scope: scope.to_string(),
            message: said().unwrap_or_else(|| "HTTP 403".to_string()),
        }),
        409 => Err(CloudError::Conflict {
            message: said().unwrap_or_else(|| "HTTP 409".to_string()),
        }),
        status => match said() {
            Some(message) if status != 429 => Err(CloudError::Refused { status, message }),
            _ => Err(error::error_for_status(url, status, &response.headers)),
        },
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawEntry {
    path: String,
    create_time: Option<String>,
    revision_id: String,
    revision_create_time: Option<String>,
    state: String,
    etag: String,
    value: Value,
    users: Vec<String>,
    attributes: Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawStore {
    create_time: Option<String>,
    state: String,
    expire_time: Option<String>,
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRef {
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawRevision {
    revision_id: String,
    revision_create_time: Option<String>,
    state: String,
    etag: String,
}

fn scope_of_path(path: &str) -> Option<&str> {
    let (_, rest) = path.split_once("/scopes/")?;
    rest.split('/').next()
}

pub(super) fn parse_entry(
    value: Value,
    fallback_scope: &str,
    key: &str,
) -> Result<Entry, CloudError> {
    let raw: RawEntry = serde_json::from_value(value)?;
    Ok(Entry {
        scope: scope_of_path(&raw.path)
            .unwrap_or(fallback_scope)
            .to_string(),
        key: key.to_string(),
        value: raw.value,
        etag: raw.etag,
        revision_id: raw.revision_id,
        create_time: raw.create_time,
        revision_create_time: raw.revision_create_time,
        deleted: raw.state == "DELETED",
        users: raw
            .users
            .iter()
            .filter_map(|u| u.strip_prefix("users/")?.parse().ok())
            .collect(),
        attributes: raw.attributes,
    })
}

fn next_token(value: &Value) -> Option<String> {
    value
        .get("nextPageToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn list_of<'a>(value: &'a Value, keys: &[&str]) -> &'a [Value] {
    keys.iter()
        .find_map(|k| value.get(*k)?.as_array())
        .map_or(&[], Vec::as_slice)
}

pub(super) fn parse_stores(value: &Value) -> Result<Page<DataStoreInfo>, CloudError> {
    let mut items = Vec::new();
    for v in list_of(value, &["dataStores"]) {
        let raw: RawStore = serde_json::from_value(v.clone())?;
        items.push(DataStoreInfo {
            id: raw.id,
            create_time: raw.create_time,
            deleted: raw.state == "DELETED",
            expire_time: raw.expire_time,
        });
    }
    Ok(Page {
        items,
        next_page_token: next_token(value),
    })
}

pub(super) fn parse_refs(value: &Value, scope: &str) -> Result<Page<EntryRef>, CloudError> {
    let mut items = Vec::new();
    for v in list_of(value, &["dataStoreEntries", "entries"]) {
        let raw: RawRef = serde_json::from_value(v.clone())?;
        let (scope, key) = match raw.id.split_once('/') {
            Some((s, k)) if scope == ALL_SCOPES => (s.to_string(), k.to_string()),
            _ => (scope.to_string(), raw.id),
        };
        items.push(EntryRef { scope, key });
    }
    Ok(Page {
        items,
        next_page_token: next_token(value),
    })
}

pub(super) fn parse_revisions(value: &Value) -> Result<Page<Revision>, CloudError> {
    let mut items = Vec::new();
    for v in list_of(value, &["dataStoreEntries"]) {
        let raw: RawRevision = serde_json::from_value(v.clone())?;
        items.push(Revision {
            revision_id: raw.revision_id,
            revision_create_time: raw.revision_create_time,
            etag: raw.etag,
            deleted: raw.state == "DELETED",
        });
    }
    Ok(Page {
        items,
        next_page_token: next_token(value),
    })
}
