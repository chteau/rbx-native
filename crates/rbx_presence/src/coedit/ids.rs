//! Instance identity for co-editing (see [`crate::coedit`]).

use std::collections::HashMap;

use rbx_dom::{Ref, UniqueId, Variant, WeakDom};
use sha2::{Digest, Sha256};

use crate::paths;

/// `UniqueId` as 32 lowercase hex: index (8), time (8), random as u64 (16).
/// `None` for the all-zero id (unset).
pub fn unique_hex(id: &UniqueId) -> Option<String> {
    if id.index == 0 && id.time == 0 && id.random == 0 {
        return None;
    }
    Some(format!(
        "{:08x}{:08x}{:016x}",
        id.index, id.time, id.random as u64
    ))
}

/// The inverse of [`unique_hex`]; `None` for anything but 32 hex digits.
pub fn parse_unique(hex: &str) -> Option<UniqueId> {
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(UniqueId {
        index: u32::from_str_radix(&hex[0..8], 16).ok()?,
        time: u32::from_str_radix(&hex[8..16], 16).ok()?,
        random: u64::from_str_radix(&hex[16..32], 16).ok()? as i64,
    })
}

/// A fresh random `UniqueId` (getrandom; index and time random too, never all zero).
pub fn fresh_unique() -> UniqueId {
    loop {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).expect("system randomness unavailable");
        let id = UniqueId {
            index: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            time: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            random: i64::from_le_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
        };
        if unique_hex(&id).is_some() {
            return id;
        }
    }
}

/// `"p"` + first 31 hex of SHA-256(`"rbx-coedit/path/v1\n" + path + "#" + n`),
/// `n` the instance's index among same-named siblings (0 for the first).
pub fn path_id(path: &str, n: usize) -> String {
    let digest = Sha256::digest(format!("rbx-coedit/path/v1\n{path}#{n}").as_bytes());
    format!("p{}", &hex::encode(digest)[..31])
}

/// Ref ↔ id for one editor's session. Entries are never dropped, so an
/// instance an undo brings back keeps its id.
#[derive(Default, Debug, Clone)]
pub struct IdMap {
    by_ref: HashMap<Ref, String>,
    by_id: HashMap<String, Ref>,
}

fn property_id(dom: &WeakDom, r: Ref) -> Option<String> {
    match dom.get(r)?.properties().get("UniqueId")? {
        Variant::UniqueId(u) => unique_hex(u),
        _ => None,
    }
}

impl IdMap {
    /// Ids for every instance in `dom` now: its `UniqueId` property
    /// (`Variant::UniqueId`, non-zero), else [`path_id`] of its path.
    pub fn build(dom: &WeakDom) -> IdMap {
        let mut map = IdMap::default();
        map.walk(dom, dom.root_refs(), None);
        map
    }

    fn walk(&mut self, dom: &WeakDom, refs: &[Ref], parent_path: Option<&str>) {
        for (i, &r) in refs.iter().enumerate() {
            let Some(inst) = dom.get(r) else { continue };
            let name = inst.name();
            let n = refs[..i]
                .iter()
                .filter(|&&s| dom.get(s).is_some_and(|x| x.name() == name))
                .count();
            let path = match parent_path {
                Some(p) => format!("{p}.{}", paths::escape(name)),
                None => paths::escape(name),
            };
            let id = property_id(dom, r).unwrap_or_else(|| path_id(&path, n));
            self.insert(r, id);
            self.walk(dom, inst.children(), Some(&path));
        }
    }

    /// The id of `r`, assigning one first if it has none: its `UniqueId`
    /// property if set, else a [`fresh_unique`] written onto the instance's
    /// `UniqueId` property (through `get_mut`, so it logs no change).
    pub fn id_of(&mut self, dom: &mut WeakDom, r: Ref) -> Option<String> {
        if let Some(id) = self.by_ref.get(&r) {
            return Some(id.clone());
        }
        let id = match property_id(dom, r) {
            Some(id) => id,
            None => {
                let u = fresh_unique();
                let id = unique_hex(&u)?;
                dom.get_mut(r)?
                    .properties_mut()
                    .insert("UniqueId".to_string(), Variant::UniqueId(u));
                id
            }
        };
        self.insert(r, id.clone());
        Some(id)
    }

    /// The id already known for `r`, without assigning.
    pub fn known(&self, r: Ref) -> Option<&str> {
        self.by_ref.get(&r).map(String::as_str)
    }

    pub fn insert(&mut self, r: Ref, id: String) {
        self.by_id.insert(id.clone(), r);
        self.by_ref.insert(r, id);
    }

    pub fn get(&self, id: &str) -> Option<Ref> {
        self.by_id.get(id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_zero() {
        let u = UniqueId {
            index: 1,
            time: 0xdead_beef,
            random: -5,
        };
        let h = unique_hex(&u).unwrap();
        assert_eq!(h.len(), 32);
        assert_eq!(parse_unique(&h), Some(u));
        let zero = UniqueId {
            index: 0,
            time: 0,
            random: 0,
        };
        assert_eq!(unique_hex(&zero), None);
        assert_eq!(parse_unique("xyz"), None);
        assert_eq!(parse_unique(&"g".repeat(32)), None);
    }

    #[test]
    fn path_id_stable_and_distinct() {
        let a = path_id("Workspace.Part", 0);
        assert_eq!(a, path_id("Workspace.Part", 0));
        assert_ne!(a, path_id("Workspace.Part", 1));
        assert_eq!(a.len(), 32);
        assert!(a.starts_with('p'));
    }

    #[test]
    fn build_uses_unique_id_else_path() {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let a = dom.new_instance("Part", "P", Some(ws));
        let b = dom.new_instance("Part", "P", Some(ws));
        let u = fresh_unique();
        dom.set_property(a, "UniqueId", Variant::UniqueId(u))
            .unwrap();
        dom.take_changes();
        let map = IdMap::build(&dom);
        assert_eq!(map.known(a), unique_hex(&u).as_deref());
        assert_eq!(map.known(b), Some(path_id("Workspace.P", 1).as_str()));
        assert_eq!(map.known(ws), Some(path_id("Workspace", 0).as_str()));
        assert_eq!(map.get(&path_id("Workspace.P", 1)), Some(b));
    }

    #[test]
    fn id_of_assigns_without_logging() {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        dom.take_changes();
        let mut map = IdMap::default();
        assert_eq!(map.known(ws), None);
        let id = map.id_of(&mut dom, ws).unwrap();
        assert_eq!(map.id_of(&mut dom, ws), Some(id.clone()));
        match dom.get(ws).unwrap().properties().get("UniqueId") {
            Some(Variant::UniqueId(u)) => assert_eq!(unique_hex(u), Some(id.clone())),
            other => panic!("unexpected {other:?}"),
        }
        assert!(dom.take_changes().is_empty());
        let mut other = IdMap::default();
        other.insert(ws, "x".into());
        assert_eq!(other.get("x"), Some(ws));
    }
}
