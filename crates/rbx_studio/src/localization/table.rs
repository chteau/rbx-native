//! A `LocalizationTable`'s entries as its `Contents` property keeps them: the
//! JSON array `GetContents` returns, each entry an object of `key`,
//! `context`, `examples` and `source` ("each omitted when empty") and a
//! `values` map of locale ID to translation (Roblox's `LocalizationTable`
//! docs). Fields this file does not know are carried through untouched.

use std::collections::{BTreeMap, HashSet};

use rbx_dom::{Ref, Variant, WeakDom};
use serde::{Deserialize, Serialize};

use super::Found;

const TABLE_CLASS: &str = "LocalizationTable";
const SERVICE_CLASS: &str = "LocalizationService";
const CONTENTS: &str = "Contents";
/// What a fresh table's `SourceLocaleId` defaults to.
const SOURCE_LOCALE: &str = "en-us";

/// One row of a table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) context: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) examples: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) values: BTreeMap<String, String>,
    #[serde(flatten)]
    pub(crate) other: serde_json::Map<String, serde_json::Value>,
}

pub(crate) fn parse(contents: &str) -> Result<Vec<Entry>, String> {
    match contents.trim() {
        "" => Ok(Vec::new()),
        json => {
            serde_json::from_str(json).map_err(|err| format!("its Contents is not a table: {err}"))
        }
    }
}

pub(crate) fn serialize(entries: &[Entry]) -> String {
    serde_json::to_string(entries).expect("plain strings and maps always serialize")
}

fn contents(dom: &WeakDom, table: Ref) -> &str {
    match dom.get(table).and_then(|i| i.properties().get(CONTENTS)) {
        Some(Variant::String(text)) => text,
        _ => "",
    }
}

/// Every `Source` some table in the place already holds: a string there is
/// already being translated, wherever the table sits.
pub(crate) fn known_sources(dom: &WeakDom) -> HashSet<String> {
    let mut known = HashSet::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend(instance.children());
        if instance.class() == TABLE_CLASS {
            let entries = parse(contents(dom, referent)).unwrap_or_default();
            known.extend(entries.into_iter().map(|entry| entry.source));
        }
    }
    known
}

/// One entry per string not in `known`, the first place it was found as
/// its context: one translation of "Play" serves every button that says it.
pub(crate) fn new_entries(found: &[Found], known: &HashSet<String>) -> Vec<Entry> {
    let mut seen = HashSet::new();
    found
        .iter()
        .filter(|found| !known.contains(&found.text) && seen.insert(found.text.as_str()))
        .map(|found| Entry {
            source: found.text.clone(),
            context: found.context.clone(),
            ..Entry::default()
        })
        .collect()
}

/// Writes `found`'s new strings into the place's table: the first
/// `LocalizationTable` under `LocalizationService`, made (with the service,
/// if need be) when there is none. Returns the table and how many entries
/// it gained; a table whose `Contents` does not parse is left alone.
pub(crate) fn write(dom: &mut WeakDom, found: &[Found]) -> Result<(Ref, usize), String> {
    let added = new_entries(found, &known_sources(dom));
    let service = dom
        .root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == SERVICE_CLASS));
    let table = service.and_then(|service| {
        dom.get(service)?
            .children()
            .iter()
            .copied()
            .find(|&r| dom.get(r).is_some_and(|i| i.class() == TABLE_CLASS))
    });
    if added.is_empty() {
        return table
            .map(|table| (table, 0))
            .ok_or_else(|| "every string is already in a table".to_string());
    }
    let mut entries = match table {
        Some(table) => parse(contents(dom, table))?,
        None => Vec::new(),
    };
    let table = match table {
        Some(table) => table,
        None => {
            let service =
                service.unwrap_or_else(|| dom.new_instance(SERVICE_CLASS, SERVICE_CLASS, None));
            let table = dom.new_instance(TABLE_CLASS, TABLE_CLASS, Some(service));
            dom.set_property(
                table,
                "SourceLocaleId",
                Variant::String(SOURCE_LOCALE.into()),
            )
            .map_err(|err| err.to_string())?;
            table
        }
    };
    let count = added.len();
    entries.extend(added);
    dom.set_property(table, CONTENTS, Variant::String(serialize(&entries)))
        .map_err(|err| err.to_string())?;
    Ok((table, count))
}
