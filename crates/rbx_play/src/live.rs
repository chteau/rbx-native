//! One side's live game tree (server or client) as an `rbx_dom::WeakDom`
//! the editor's Explorer and Properties can show, kept in step with the
//! probe's `Tree`, `Diff` and `Props` messages.
//!
//! Shape: like a loaded place, the top-level instances are the services
//! (a `Node` with `parent == 0`); there is no `DataModel` instance. Every
//! mutation goes through `WeakDom`'s own methods, so its change log
//! (`take_changes`) tells the editor what to refresh.

use std::collections::{BTreeMap, HashMap};

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::protocol::{Move, Node, Rename};

#[derive(Debug, Default)]
pub struct LiveTree {
    pub dom: WeakDom,
    ids: HashMap<u64, Ref>,
    refs: HashMap<Ref, u64>,
    /// Nodes whose parent has not arrived yet, keyed by that parent's id.
    pending: HashMap<u64, Vec<Node>>,
}

fn decode_attribute(json: &serde_json::Value, db: &ReflectionDatabase) -> Option<rbx_dom::Variant> {
    use rbx_dom::Variant;
    match json {
        serde_json::Value::Bool(b) => Some(Variant::Bool(*b)),
        serde_json::Value::String(s) => Some(Variant::String(s.clone())),
        serde_json::Value::Number(n) => n.as_f64().map(Variant::Float64),
        serde_json::Value::Object(map) => {
            let value_type = match map.get("t")?.as_str()? {
                "V2" => "Vector2",
                "V3" => "Vector3",
                "CF" => "CFrame",
                "C3" => "Color3",
                "BC" => "BrickColor",
                "U" => "UDim",
                "U2" => "UDim2",
                "R" => "Rect",
                "NR" => "NumberRange",
                "NS" => "NumberSequence",
                "CS" => "ColorSequence",
                "n" => "double",
                _ => return None,
            };
            crate::value::decode(json, value_type, db, &|_| None)
        }
        _ => None,
    }
}

impl LiveTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops everything (a `Tree` message with `reset`).
    pub fn clear(&mut self) {
        let roots: Vec<Ref> = self.dom.root_refs().to_vec();
        for root in roots {
            self.dom.remove(root);
        }
        self.ids.clear();
        self.refs.clear();
        self.pending.clear();
    }

    fn parent_ref(&self, parent: u64) -> Option<Option<Ref>> {
        if parent == 0 {
            Some(None)
        } else {
            self.ids.get(&parent).map(|r| Some(*r))
        }
    }

    fn set_name(&mut self, referent: Ref, name: &str) {
        let _ = self.dom.set_name(referent, name);
        let _ = self
            .dom
            .set_property(referent, "Name", rbx_dom::Variant::String(name.to_string()));
    }

    fn add_one(&mut self, node: &Node) {
        if let Some(&existing) = self.ids.get(&node.id) {
            self.set_name(existing, &node.name);
            if let Some(parent) = self.parent_ref(node.parent) {
                if self.dom.parent(existing) != parent {
                    self.dom.set_parent(existing, parent);
                }
            }
            return;
        }
        let Some(parent) = self.parent_ref(node.parent) else {
            self.pending
                .entry(node.parent)
                .or_default()
                .push(node.clone());
            return;
        };
        let referent = self.dom.new_instance(&node.class, &node.name, parent);
        self.set_name(referent, &node.name);
        self.ids.insert(node.id, referent);
        self.refs.insert(referent, node.id);
        if let Some(children) = self.pending.remove(&node.id) {
            for child in &children {
                self.add_one(child);
            }
        }
    }

    fn forget_gone(&mut self) {
        let dom = &self.dom;
        self.ids.retain(|_, r| dom.get(*r).is_some());
        self.refs.retain(|r, _| dom.get(*r).is_some());
    }

    /// Adds nodes (from `Tree` chunks or `Diff::added`). A node whose id is
    /// already known is renamed/reparented instead (and its class kept). A
    /// node whose parent is unknown waits until that parent arrives; a
    /// later `clear` drops it. New instances get `Name` only; other
    /// properties come from `Props`.
    pub fn add(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.add_one(node);
        }
    }

    /// Removes instances and their subtrees (unknown ids are ignored).
    pub fn remove(&mut self, ids: &[u64]) {
        for id in ids {
            self.pending.remove(id);
            if let Some(&referent) = self.ids.get(id) {
                self.dom.remove(referent);
            }
        }
        self.forget_gone();
    }

    pub fn rename(&mut self, renames: &[Rename]) {
        for rename in renames {
            if let Some(&referent) = self.ids.get(&rename.id) {
                self.set_name(referent, &rename.name);
            }
        }
    }

    /// Reparents (`parent == 0` → a top-level instance). A move under an
    /// unknown parent removes the instance (it left the replicated tree).
    pub fn reparent(&mut self, moves: &[Move]) {
        for mv in moves {
            let Some(&referent) = self.ids.get(&mv.id) else {
                continue;
            };
            match self.parent_ref(mv.parent) {
                Some(parent) => self.dom.set_parent(referent, parent),
                None => self.remove(&[mv.id]),
            }
        }
    }

    /// Applies a `Props` message: each value decoded with
    /// `crate::value::decode` against the instance's class's reflected
    /// `value_type` (properties the database does not know, or values that
    /// do not decode, are skipped); attributes (when `Some`) replace the
    /// `AttributesSerialize` blob via `rbx_dom::attributes::encode`; tags
    /// (when `Some`) replace `Tags`. Unknown ids are ignored.
    pub fn apply_props(
        &mut self,
        id: u64,
        values: &std::collections::BTreeMap<String, serde_json::Value>,
        attributes: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
        tags: Option<&[String]>,
        db: &ReflectionDatabase,
    ) {
        use rbx_dom::Variant;
        let Some(&referent) = self.ids.get(&id) else {
            return;
        };
        let Some(class) = self.dom.get(referent).map(|i| i.class().to_string()) else {
            return;
        };
        for (name, json) in values {
            let Some(descriptor) = db.resolve_property(&class, name) else {
                continue;
            };
            let value_type = descriptor.value_type.to_string();
            let ids = &self.ids;
            let Some(value) =
                crate::value::decode(json, &value_type, db, &|i| ids.get(&i).copied())
            else {
                continue;
            };
            let _ = self.dom.set_property(referent, name, value);
        }
        if let Some(attributes) = attributes {
            let decoded: BTreeMap<String, Variant> = attributes
                .iter()
                .filter_map(|(k, v)| decode_attribute(v, db).map(|v| (k.clone(), v)))
                .collect();
            if let Some(raw) = rbx_dom::attributes::encode(&decoded, |_| None) {
                let _ = self.dom.set_property(
                    referent,
                    "AttributesSerialize",
                    Variant::Unknown { type_id: 0x01, raw },
                );
            }
        }
        if let Some(tags) = tags {
            let names: Vec<&str> = tags.iter().map(String::as_str).collect();
            let _ = self.dom.set_property(
                referent,
                "Tags",
                Variant::String(rbx_dom::attributes::encode_tags(&names)),
            );
        }
    }

    pub fn ref_of(&self, id: u64) -> Option<Ref> {
        self.ids.get(&id).copied()
    }

    pub fn id_of(&self, referent: Ref) -> Option<u64> {
        self.refs.get(&referent).copied()
    }

    /// Number of instances in the tree (pending orphans excluded).
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
