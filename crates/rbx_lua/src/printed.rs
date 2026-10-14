//! What one `print` call said, kept structured: Studio's Output window shows a
//! printed table as an expandable tree of its keys and values rather than the
//! `table: 0x…` string `tostring` gives, so a table argument is walked here,
//! while the VM still holds it, into a [`TableDump`] the editor can draw.

use std::collections::HashSet;

use mlua::{Table, Value};

/// Past this depth a nested table is shown by address only: a deep tree is
/// unreadable in a log row long before it is expensive to walk.
const MAX_DEPTH: usize = 8;

/// Past this many entries a table's remaining ones are counted, not walked.
const MAX_ENTRIES: usize = 200;

/// One `print` argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintPart {
    /// Anything `tostring` already says well: numbers, strings, Instances, a
    /// table with its own `__tostring`.
    Text(String),
    Table(TableDump),
}

/// A printed table's contents, keys in the order [`dump`] sorts them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDump {
    /// `0x…`, the same address `tostring` puts after `table: `.
    pub address: String,
    pub entries: Vec<TableEntry>,
    /// Entries past [`MAX_ENTRIES`], not walked.
    pub more: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEntry {
    /// The key as Studio writes it inside its brackets: `"name"` or `1`.
    pub key: String,
    pub value: PrintValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintValue {
    /// A scalar, strings quoted: `"hi"`, `3`, `true`, `Part`.
    Text(String),
    Table(TableDump),
    /// A table already open further up this branch (or past [`MAX_DEPTH`]),
    /// named by its address instead of walked again.
    Reference(String),
}

/// One `print` argument as a [`PrintPart`].
pub(crate) fn part(value: &Value) -> mlua::Result<PrintPart> {
    match value {
        Value::Table(table) if !has_tostring(table) => {
            Ok(PrintPart::Table(dump(table, &mut HashSet::new(), 0)?))
        }
        other => Ok(PrintPart::Text(other.to_string()?)),
    }
}

/// The text `print` writes for these parts: what `tostring` gives each one,
/// space-separated — the shape a plain log reader sees.
pub(crate) fn text(parts: &[PrintPart]) -> String {
    let words: Vec<String> = parts
        .iter()
        .map(|part| match part {
            PrintPart::Text(text) => text.clone(),
            PrintPart::Table(table) => format!("table: {}", table.address),
        })
        .collect();
    words.join(" ")
}

fn has_tostring(table: &Table) -> bool {
    table.metatable().is_some_and(|meta| {
        meta.raw_get::<Value>("__tostring")
            .is_ok_and(|v| !v.is_nil())
    })
}

fn address(table: &Table) -> String {
    format!("0x{:016x}", table.to_pointer() as usize)
}

/// Walks `table`; `open` holds the tables on the current branch, so a cycle
/// ends in a [`PrintValue::Reference`] instead of recursing forever.
fn dump(table: &Table, open: &mut HashSet<usize>, depth: usize) -> mlua::Result<TableDump> {
    let pointer = table.to_pointer() as usize;
    open.insert(pointer);
    let mut pairs: Vec<(Value, Value)> = Vec::new();
    let mut more = 0;
    for pair in table.pairs::<Value, Value>() {
        if pairs.len() < MAX_ENTRIES {
            pairs.push(pair?);
        } else {
            more += 1;
        }
    }
    pairs.sort_by(|(a, _), (b, _)| key_order(a, b));

    let mut entries = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        let key = match &key {
            Value::String(text) => format!("{:?}", text.to_string_lossy()),
            other => other.to_string()?,
        };
        let value = match &value {
            Value::Table(inner) if !has_tostring(inner) => {
                let inner_pointer = inner.to_pointer() as usize;
                if open.contains(&inner_pointer) || depth + 1 >= MAX_DEPTH {
                    PrintValue::Reference(address(inner))
                } else {
                    PrintValue::Table(dump(inner, open, depth + 1)?)
                }
            }
            Value::String(text) => PrintValue::Text(format!("{:?}", text.to_string_lossy())),
            other => PrintValue::Text(other.to_string()?),
        };
        entries.push(TableEntry { key, value });
    }
    open.remove(&pointer);
    Ok(TableDump {
        address: address(table),
        entries,
        more,
    })
}

/// Numbers first, ascending, then strings alphabetically, then everything
/// else in the order `next` gave it — an array reads `[1]`, `[2]`, … and a
/// record reads alphabetically, whatever the hash order was.
fn key_order(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn rank(value: &Value) -> u8 {
        match value {
            Value::Integer(_) | Value::Number(_) => 0,
            Value::String(_) => 1,
            _ => 2,
        }
    }
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.as_bytes().cmp(&b.as_bytes()),
        _ if rank(a) == 0 && rank(b) == 0 => {
            let (a, b) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
            a.partial_cmp(&b).unwrap_or(Ordering::Equal)
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

#[cfg(test)]
mod tests;
