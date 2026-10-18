//! Per-attribute merging of `AttributesSerialize` blobs.

use std::collections::BTreeMap;

use super::{settle_side, Found, Pick};
use crate::attributes::{encode, split_entries};
use crate::merge::{Resolution, Side, Target};
use crate::{Ref, Variant};

type Entry = (String, Variant, Vec<u8>);

/// One side's blob, its `Instance` entries translated to output referents.
pub(super) struct Blob<'a> {
    original: Option<&'a Variant>,
    entries: Vec<Entry>,
    /// Translation left every byte as stored and dropped nothing.
    pristine: bool,
}

/// `None` when the blob cannot be read whole.
pub(super) fn read<'a>(
    side: Side,
    value: Option<&'a Variant>,
    out: &dyn Fn(Side, Ref) -> Option<Ref>,
) -> Option<Blob<'a>> {
    let Some(v) = value else {
        return Some(Blob {
            original: None,
            entries: Vec::new(),
            pristine: true,
        });
    };
    let mut pristine = true;
    let mut entries = Vec::new();
    for (name, decoded, mut raw) in split_entries(v)? {
        if let Variant::Ref(r) = decoded {
            let len = raw.len();
            match out(side, r) {
                Some(nr) if len >= 4 => {
                    let bytes = nr.value().to_le_bytes();
                    if raw[len - 4..] != bytes {
                        pristine = false;
                        raw[len - 4..].copy_from_slice(&bytes);
                    }
                    entries.push((name, Variant::Ref(nr), raw));
                }
                _ => pristine = false,
            }
        } else {
            entries.push((name, decoded, raw));
        }
    }
    Some(Blob {
        original: Some(v),
        entries,
        pristine,
    })
}

fn blob_variant(bytes: Vec<u8>) -> Variant {
    match String::from_utf8(bytes) {
        Ok(text) => Variant::String(text),
        Err(err) => Variant::Unknown {
            type_id: 0x01,
            raw: err.into_bytes(),
        },
    }
}

fn custom_entry(name: &str, value: Variant) -> Option<Entry> {
    let mut map = BTreeMap::new();
    map.insert(name.to_string(), value);
    let bytes = encode(&map, |_| None)?;
    split_entries(&blob_variant(bytes))?.into_iter().next()
}

fn get<'a>(blob: Option<&'a Blob<'_>>, name: &str) -> Option<&'a Entry> {
    blob?.entries.iter().find(|e| e.0 == name)
}

fn same(a: Option<&Entry>, b: Option<&Entry>) -> bool {
    a.map(|e| &e.2) == b.map(|e| &e.2)
}

pub(super) fn merge(
    base: Option<&Blob<'_>>,
    ours: &Blob<'_>,
    theirs: &Blob<'_>,
    choose: &dyn Fn(&Target) -> Option<Resolution>,
    found: &mut Vec<Found>,
) -> Option<Variant> {
    let has_base = base.is_some();
    let mut names: Vec<&str> = Vec::new();
    let all = ours
        .entries
        .iter()
        .chain(&theirs.entries)
        .chain(base.into_iter().flat_map(|b| &b.entries));
    for e in all {
        if !names.contains(&e.0.as_str()) {
            names.push(&e.0);
        }
    }
    let mut chosen: Vec<Entry> = Vec::new();
    for name in names {
        let (b, o, t) = (
            get(base, name),
            get(Some(ours), name),
            get(Some(theirs), name),
        );
        let pick = if same(o, t) {
            Pick::Agreed(o)
        } else if has_base && same(o, b) {
            Pick::Agreed(t)
        } else if has_base && same(t, b) {
            Pick::Agreed(o)
        } else {
            Pick::Conflict
        };
        let entry = match pick {
            Pick::Agreed(e) => e.cloned(),
            Pick::Conflict => {
                let target = Target::Attribute(name.to_string());
                let resolution = choose(&target);
                found.push(Found {
                    target,
                    base: b.map(|e| e.1.clone()),
                    ours: o.map(|e| e.1.clone()),
                    theirs: t.map(|e| e.1.clone()),
                });
                match resolution {
                    Some(Resolution::Custom(v)) => custom_entry(name, v),
                    r => settle_side(r, b, o, t).cloned(),
                }
            }
        };
        chosen.extend(entry);
    }
    for blob in base.into_iter().chain([ours, theirs]) {
        if blob.pristine
            && blob.original.is_some()
            && blob.entries.len() == chosen.len()
            && blob.entries.iter().zip(&chosen).all(|(a, b)| a.2 == b.2)
        {
            return blob.original.cloned();
        }
    }
    if chosen.is_empty() {
        return None;
    }
    let count = u32::try_from(chosen.len()).ok()?;
    let mut bytes = count.to_le_bytes().to_vec();
    for e in chosen {
        bytes.extend(e.2);
    }
    Some(blob_variant(bytes))
}
