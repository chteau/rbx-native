//! Shared string table construction, the encode counterpart of `chunks::sstr`.
//!
//! Real files mix `Variant::String` (a SharedString entry that happened to decode as
//! valid UTF-8, including an empty one) and `Variant::Unknown { type_id: 0x1C, .. }`
//! (binary payloads like mesh data) on sibling instances of the very same property,
//! because the wire type is fixed per (class, property) but each entry's *content*
//! decides which `Variant` shape the reader produces. Both shapes are interned here
//! under one dedup table so the property encoder can write a single, consistent index
//! stream regardless of which shape a given instance happened to come back as.

use std::collections::HashMap;

use md5::{Digest, Md5};
use rbx_dom::Variant;

use super::writer::Writer;

// Exactly as `chunks::prop::scalar` and `serialize::prop`'s local `type_id` module
// define it; kept as a private copy here for the same reason they do.
pub(super) const SHARED_STRING_TYPE_ID: u8 = 0x1C;

/// Deduplicated shared-string payloads, keyed by content (MD5, matching how Roblox's
/// own writer dedups) and ordered by first insertion, which becomes the wire index.
pub(crate) struct SharedStringTable {
    entries: Vec<Vec<u8>>,
    index: HashMap<[u8; 16], u32>,
}

impl SharedStringTable {
    pub(crate) fn new() -> Self {
        SharedStringTable {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Interns `bytes`, returning the existing index if this exact payload was
    /// already seen, or a fresh one at the end of the table otherwise.
    pub(crate) fn intern(&mut self, bytes: &[u8]) -> u32 {
        let hash = md5_of(bytes);
        if let Some(&existing) = self.index.get(&hash) {
            return existing;
        }
        let index = self.entries.len() as u32;
        self.entries.push(bytes.to_vec());
        self.index.insert(hash, index);
        index
    }

    /// Looks up a payload already interned by an earlier `intern` call. `None` means
    /// this exact content never went through the table-building pass.
    pub(crate) fn index_of(&self, bytes: &[u8]) -> Option<u32> {
        self.index.get(&md5_of(bytes)).copied()
    }

    /// Encodes the SSTR chunk payload: version, count, then MD5 + length-prefixed
    /// bytes per entry, matching `chunks::sstr::parse`.
    pub(crate) fn write(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.i32(0); // version
        writer.length(self.entries.len());
        for entry in &self.entries {
            writer.bytes(&md5_of(entry));
            writer.sized_bytes(entry);
        }
        writer.into_bytes()
    }
}

fn md5_of(bytes: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Scans one (class, property) group and interns every resolvable payload, but only
/// if the group actually carries the SharedString wire type: a plain, ordinary String
/// property must never be pulled into the table (it has nothing to do with SSTR).
/// The tell is the same one `serialize::prop::encode` uses to pick the wire type: at
/// least one instance decoded to `Unknown { type_id: 0x1C, .. }` rather than a plain
/// `Variant::String`.
pub(crate) fn collect_group(table: &mut SharedStringTable, values: &[Option<Variant>]) {
    let is_shared_string_group = values.iter().any(|value| {
        matches!(
            value,
            Some(Variant::Unknown { type_id, .. }) if *type_id == SHARED_STRING_TYPE_ID
        )
    });
    if !is_shared_string_group {
        return;
    }

    for value in values {
        match value {
            Some(Variant::String(text)) => {
                table.intern(text.as_bytes());
            }
            Some(Variant::Unknown { type_id, raw }) if *type_id == SHARED_STRING_TYPE_ID => {
                table.intern(raw);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_payloads_dedup_to_one_entry() {
        let mut table = SharedStringTable::new();
        assert_eq!(table.intern(b"same"), 0);
        assert_eq!(table.intern(b"same"), 0);
        assert_eq!(table.intern(b"same"), 0);
        assert_eq!(table.entries.len(), 1);
    }

    #[test]
    fn distinct_payloads_get_separate_entries() {
        let mut table = SharedStringTable::new();
        assert_eq!(table.intern(b"a"), 0);
        assert_eq!(table.intern(b"b"), 1);
        assert_eq!(table.entries.len(), 2);
    }

    #[test]
    fn collect_group_ignores_a_plain_string_only_group() {
        let mut table = SharedStringTable::new();
        let values = vec![
            Some(Variant::String("hello".to_owned())),
            Some(Variant::String("hello".to_owned())),
        ];
        collect_group(&mut table, &values);
        assert_eq!(table.entries.len(), 0);
    }

    #[test]
    fn collect_group_interns_the_mix_of_string_and_unknown_shapes() {
        let mut table = SharedStringTable::new();
        let values = vec![
            Some(Variant::String(String::new())),
            Some(Variant::Unknown {
                type_id: SHARED_STRING_TYPE_ID,
                raw: b"mesh-bytes".to_vec(),
            }),
            Some(Variant::Unknown {
                type_id: SHARED_STRING_TYPE_ID,
                raw: b"mesh-bytes".to_vec(),
            }),
        ];
        collect_group(&mut table, &values);
        // Empty string + one distinct binary payload (seen twice) = 2 unique entries.
        assert_eq!(table.entries.len(), 2);
        assert_eq!(table.index_of(b""), Some(0));
        assert_eq!(table.index_of(b"mesh-bytes"), Some(1));
    }
}
