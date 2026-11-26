//! The data behind a Local Run's DataStores and its purchases: every
//! store's keys with all their versions, and what each user owns. One per
//! universe, kept in a JSON file per place when the run has one (written
//! after every change) or in memory only.
//!
//! Values are stored as the JSON text the Luau side encoded; the Luau side
//! keeps the API's rules (key and value limits, budgets, errors).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A bucket's name in the map: `d` or `o` (ordered), the store's name and
/// its scope, separated by `\u{1}`.
const SEP: char = '\u{1}';

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Data {
    #[serde(default)]
    stores: BTreeMap<String, Bucket>,
    /// User id → `pass:<id>` / `asset:<id>` / `product:<id>` items.
    #[serde(default)]
    owned: BTreeMap<String, BTreeSet<String>>,
    /// The last version number handed out.
    #[serde(default)]
    serial: u64,
    #[serde(skip)]
    file: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
struct Bucket {
    created: i64,
    updated: i64,
    keys: BTreeMap<String, Vec<Version>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Version {
    pub id: String,
    /// Unix milliseconds.
    pub created: i64,
    pub deleted: bool,
    /// The JSON text; `None` for a deletion.
    pub value: Option<String>,
    /// JSON arrays/objects as the Luau side encoded them.
    pub users: String,
    pub meta: String,
}

/// A key's current state, as `GetAsync` and friends report it.
pub(crate) struct Entry {
    pub version: Version,
    /// When the key was first written (since it was last deleted).
    pub created: i64,
}

impl Data {
    /// The store kept in `file`, empty when there is none yet or it can't
    /// be read; written back there after every change.
    pub fn open(file: Option<PathBuf>) -> Data {
        let mut data: Data = file
            .as_deref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        data.file = file;
        data
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let Ok(text) = serde_json::to_vec(self) else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let partial = file.with_extension("partial");
        if std::fs::write(&partial, text).is_ok() {
            let _ = std::fs::rename(&partial, file);
        }
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The current entry for `key`, `None` when it has no value.
    pub fn get(&self, bucket: &str, key: &str) -> Option<Entry> {
        let versions = self.stores.get(bucket)?.keys.get(key)?;
        let last = versions.last().filter(|v| !v.deleted)?;
        Some(Entry {
            version: last.clone(),
            created: since_deleted(versions),
        })
    }

    /// The version named `id`, whether it's current or not.
    pub fn version(&self, bucket: &str, key: &str, id: &str) -> Option<Entry> {
        let versions = self.stores.get(bucket)?.keys.get(key)?;
        let at = versions.iter().position(|v| v.id == id)?;
        Some(Entry {
            version: versions[at].clone(),
            created: since_deleted(&versions[..=at]),
        })
    }

    /// The version that was current at `time` (Unix ms).
    pub fn version_at(&self, bucket: &str, key: &str, time: i64) -> Option<Entry> {
        let versions = self.stores.get(bucket)?.keys.get(key)?;
        let at = versions.iter().rposition(|v| v.created <= time)?;
        Some(Entry {
            version: versions[at].clone(),
            created: since_deleted(&versions[..=at]),
        })
    }

    pub fn versions(&self, bucket: &str, key: &str) -> Vec<Version> {
        self.stores
            .get(bucket)
            .and_then(|b| b.keys.get(key))
            .cloned()
            .unwrap_or_default()
    }

    fn next_id(&mut self, now: i64) -> String {
        self.serial += 1;
        let ticks = (now.max(0) as u64) * 10_000;
        format!("{ticks:016X}.{:010}.{ticks:016X}.01", self.serial)
    }

    /// Writes a new version of `key` (`value: None` deletes it) and returns
    /// the entry it makes.
    pub fn put(
        &mut self,
        bucket: &str,
        key: &str,
        value: Option<String>,
        users: String,
        meta: String,
        now: i64,
    ) -> Entry {
        let id = self.next_id(now);
        let store = self.stores.entry(bucket.to_owned()).or_insert(Bucket {
            created: now,
            updated: now,
            keys: BTreeMap::new(),
        });
        store.updated = now;
        let versions = store.keys.entry(key.to_owned()).or_default();
        // Two writes in the same millisecond still go in order.
        let created = versions.last().map_or(now, |v| v.created.max(now));
        versions.push(Version {
            id,
            created,
            deleted: value.is_none(),
            value,
            users,
            meta,
        });
        let entry = Entry {
            version: versions.last().cloned().expect("just pushed"),
            created: since_deleted(versions),
        };
        self.save();
        entry
    }

    /// Deletes one version; `false` when there is no such version or it's
    /// the current one (Roblox keeps the current version).
    pub fn remove_version(&mut self, bucket: &str, key: &str, id: &str) -> bool {
        let Some(versions) = self
            .stores
            .get_mut(bucket)
            .and_then(|b| b.keys.get_mut(key))
        else {
            return false;
        };
        let Some(at) = versions.iter().position(|v| v.id == id) else {
            return false;
        };
        if at + 1 == versions.len() {
            return false;
        }
        versions.remove(at);
        self.save();
        true
    }

    /// Key names in a store (every scope's, as `scope/key`, for
    /// `scope: None`) starting with `prefix`, sorted.
    pub fn keys(
        &self,
        name: &str,
        scope: Option<&str>,
        prefix: &str,
        deleted: bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for (bucket, store) in &self.stores {
            let mut parts = bucket.split(SEP);
            let (Some("d"), Some(n), Some(s)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            if n != name || scope.is_some_and(|want| want != s) {
                continue;
            }
            for (key, versions) in &store.keys {
                if !deleted && versions.last().is_none_or(|v| v.deleted) {
                    continue;
                }
                let full = if scope.is_some() {
                    key.clone()
                } else {
                    format!("{s}/{key}")
                };
                if full.starts_with(prefix) {
                    out.push(full);
                }
            }
        }
        out.sort();
        out
    }

    /// The (non-ordered) stores whose name starts with `prefix`: name,
    /// created and updated (Unix ms), sorted by name.
    pub fn stores(&self, prefix: &str) -> Vec<(String, i64, i64)> {
        let mut out: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for (bucket, store) in &self.stores {
            let mut parts = bucket.split(SEP);
            let (Some("d"), Some(name)) = (parts.next(), parts.next()) else {
                continue;
            };
            if !name.starts_with(prefix) {
                continue;
            }
            let times = out
                .entry(name.to_owned())
                .or_insert((store.created, store.updated));
            times.0 = times.0.min(store.created);
            times.1 = times.1.max(store.updated);
        }
        out.into_iter().map(|(n, (c, u))| (n, c, u)).collect()
    }

    /// Every current entry of an ordered store: key and JSON value.
    pub fn entries(&self, bucket: &str) -> Vec<(String, String)> {
        let Some(store) = self.stores.get(bucket) else {
            return Vec::new();
        };
        store
            .keys
            .iter()
            .filter_map(|(key, versions)| {
                let last = versions.last().filter(|v| !v.deleted)?;
                Some((key.clone(), last.value.clone()?))
            })
            .collect()
    }

    pub fn owns(&self, user: i64, item: &str) -> bool {
        self.owned
            .get(&user.to_string())
            .is_some_and(|items| items.contains(item))
    }

    pub fn grant(&mut self, user: i64, item: &str) {
        self.owned
            .entry(user.to_string())
            .or_default()
            .insert(item.to_owned());
        self.save();
    }

    pub fn revoke(&mut self, user: i64, item: &str) {
        if let Some(items) = self.owned.get_mut(&user.to_string()) {
            items.remove(item);
            self.save();
        }
    }
}

/// When the run of versions ending the slice started: the first version
/// after the last deletion before the end.
fn since_deleted(versions: &[Version]) -> i64 {
    let end = versions.len().saturating_sub(1);
    let start = versions[..end]
        .iter()
        .rposition(|v| v.deleted)
        .map_or(0, |i| i + 1);
    versions.get(start).map_or(0, |v| v.created)
}

pub(crate) fn bucket(ordered: bool, name: &str, scope: &str) -> String {
    format!("{}{SEP}{name}{SEP}{scope}", if ordered { "o" } else { "d" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_keys_and_persistence() {
        let dir = std::env::temp_dir().join(format!("rbx_lua_store_{}", std::process::id()));
        let file = dir.join("place.json");
        let b = bucket(false, "Coins", "global");
        {
            let mut data = Data::open(Some(file.clone()));
            let first = data.put(&b, "k", Some("1".into()), "[]".into(), "{}".into(), 1000);
            data.put(&b, "k", Some("2".into()), "[]".into(), "{}".into(), 2000);
            assert_eq!(
                data.get(&b, "k").unwrap().version.value.as_deref(),
                Some("2")
            );
            assert_eq!(data.get(&b, "k").unwrap().created, 1000);
            assert_eq!(
                data.version(&b, "k", &first.version.id)
                    .unwrap()
                    .version
                    .value
                    .as_deref(),
                Some("1")
            );
            assert_eq!(
                data.version_at(&b, "k", 1500)
                    .unwrap()
                    .version
                    .value
                    .as_deref(),
                Some("1")
            );
            data.put(&b, "gone", None, "[]".into(), "{}".into(), 3000);
            data.grant(5, "pass:7");
        }
        let data = Data::open(Some(file));
        assert_eq!(data.keys("Coins", Some("global"), "", false), vec!["k"]);
        assert_eq!(
            data.keys("Coins", None, "glo", true),
            vec!["global/gone", "global/k"]
        );
        assert!(data.get(&b, "gone").is_none());
        assert!(data.owns(5, "pass:7"));
        assert_eq!(data.stores("Co").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
