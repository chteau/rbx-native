//! A script drawn as nodes and wires. The graph compiles to Luau
//! ([`codegen`]) that is written to the script's `Source`, so a place keeps
//! running in Roblox with nothing added; the graph itself is kept, as JSON,
//! in the script's [`ATTRIBUTE`] attribute, which a save, a publish and a
//! sync carry along and the engine ignores.
//!
//! Only the data and its rules live here — what may connect to what, and
//! what removing a node takes with it — so they can be tested without a
//! window. `shell::script_graph` draws and edits it.

pub(crate) mod catalog;
pub(crate) mod codegen;
pub(crate) mod import;
pub(crate) mod layout;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use catalog::{Kind, PinType};

/// Roblox reserves attribute names starting `RBX`, so the graph's is plain.
pub(crate) const ATTRIBUTE: &str = "ScriptGraph";

pub(crate) type NodeId = u32;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Graph {
    #[serde(default)]
    pub(crate) nodes: Vec<Node>,
    #[serde(default)]
    pub(crate) wires: Vec<Wire>,
    #[serde(default)]
    pub(crate) groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Node {
    pub(crate) id: NodeId,
    /// A [`catalog::Kind`]'s key. Kept as text so a graph saved by a newer
    /// build with a kind this one lacks still loads, and says so.
    pub(crate) kind: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// The literal each unwired value input holds, where it differs from
    /// its pin's default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) values: BTreeMap<String, String>,
}

/// One end of a wire: a node's pin, by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct End {
    pub(crate) node: NodeId,
    pub(crate) pin: String,
}

impl End {
    pub(crate) fn new(node: NodeId, pin: &str) -> End {
        End {
            node,
            pin: pin.to_owned(),
        }
    }
}

/// Always from an output to an input, whichever end it was dragged from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Wire {
    pub(crate) from: End,
    pub(crate) to: End,
}

/// A titled frame drawn behind the nodes it was made round. Nodes are not
/// owned by it: it is a note on the canvas, not a scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub(crate) title: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// Why [`Graph::connect`] refused a wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    NoSuchPin,
    SameNode,
    Types(PinType, PinType),
    /// A value would end up depending on itself.
    Loop,
}

impl Refused {
    pub(crate) fn message(self) -> String {
        match self {
            Refused::NoSuchPin => "Those pins cannot be wired together".into(),
            Refused::SameNode => "A node cannot be wired to itself".into(),
            Refused::Types(from, to) => format!("A {} cannot go into a {}", from.name(), to.name()),
            Refused::Loop => "That wire would make a value depend on itself".into(),
        }
    }
}

impl Graph {
    /// `None` for text that is not a graph this build can read.
    pub(crate) fn parse(text: &str) -> Option<Graph> {
        serde_json::from_str(text).ok()
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub(crate) fn kind_of(&self, id: NodeId) -> Option<&'static Kind> {
        self.node(id).and_then(|node| catalog::kind(&node.kind))
    }

    /// Adds a node of `kind` with its top-left corner at `at`.
    pub(crate) fn add(&mut self, kind: &Kind, at: [f32; 2]) -> NodeId {
        let id = self.nodes.iter().map(|node| node.id + 1).max().unwrap_or(1);
        self.nodes.push(Node {
            id,
            kind: kind.key.to_owned(),
            x: at[0],
            y: at[1],
            values: BTreeMap::new(),
        });
        id
    }

    /// Removes the nodes and every wire touching them.
    pub(crate) fn remove(&mut self, ids: &BTreeSet<NodeId>) {
        self.nodes.retain(|node| !ids.contains(&node.id));
        self.wires
            .retain(|wire| !ids.contains(&wire.from.node) && !ids.contains(&wire.to.node));
    }

    /// The wire ending on an input, if one does.
    pub(crate) fn wire_into(&self, to: &End) -> Option<&Wire> {
        self.wires.iter().find(|wire| wire.to == *to)
    }

    pub(crate) fn wires_from<'a>(&'a self, from: &'a End) -> impl Iterator<Item = &'a Wire> {
        self.wires.iter().filter(move |wire| wire.from == *from)
    }

    pub(crate) fn disconnect(&mut self, to: &End) {
        self.wires.retain(|wire| wire.to != *to);
    }

    /// The literal an unwired input holds: what was typed, or its default.
    pub(crate) fn value(&self, end: &End) -> Option<String> {
        let node = self.node(end.node)?;
        if let Some(value) = node.values.get(&end.pin) {
            return Some(value.clone());
        }
        catalog::kind(&node.kind)?
            .input(&end.pin)?
            .default
            .map(str::to_owned)
    }

    pub(crate) fn set_value(&mut self, end: &End, value: String) {
        if let Some(node) = self.node_mut(end.node) {
            node.values.insert(end.pin.clone(), value);
        }
    }

    /// Wires an output to an input. A value input takes one wire and a
    /// run-order output gives one, so either replaces what was there; a
    /// run-order input takes any number, as runs merge into it.
    pub(crate) fn connect(&mut self, from: End, to: End) -> Result<(), Refused> {
        let out = self
            .kind_of(from.node)
            .and_then(|kind| kind.output(&from.pin))
            .ok_or(Refused::NoSuchPin)?;
        let into = self
            .kind_of(to.node)
            .and_then(|kind| kind.input(&to.pin))
            .ok_or(Refused::NoSuchPin)?;
        if from.node == to.node {
            return Err(Refused::SameNode);
        }
        if !into.ty.accepts(out.ty) {
            return Err(Refused::Types(out.ty, into.ty));
        }
        if out.ty != PinType::Exec && self.feeds(to.node, from.node) {
            return Err(Refused::Loop);
        }
        match out.ty {
            PinType::Exec => self.wires.retain(|wire| wire.from != from),
            _ => self.disconnect(&to),
        }
        self.wires.push(Wire { from, to });
        Ok(())
    }

    /// Whether `node`'s values reach `target` through value wires.
    fn feeds(&self, node: NodeId, target: NodeId) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![node];
        while let Some(at) = stack.pop() {
            if at == target {
                return true;
            }
            if !seen.insert(at) {
                continue;
            }
            stack.extend(
                self.wires
                    .iter()
                    .filter(|wire| wire.from.node == at && !self.is_exec(&wire.from))
                    .map(|wire| wire.to.node),
            );
        }
        false
    }

    /// The nodes drawn wholly inside a group's frame — what moves with it.
    pub(crate) fn nodes_within(&self, group: usize) -> Vec<NodeId> {
        let Some(group) = self.groups.get(group) else {
            return Vec::new();
        };
        let frame = layout::Rect {
            x: group.x,
            y: group.y,
            w: group.w,
            h: group.h,
        };
        self.nodes
            .iter()
            .filter(|node| {
                let rect = layout::rect(self, node);
                frame.contains([rect.x, rect.y])
                    && frame.contains([rect.x + rect.w, rect.y + rect.h])
            })
            .map(|node| node.id)
            .collect()
    }

    pub(crate) fn is_exec(&self, from: &End) -> bool {
        self.kind_of(from.node)
            .and_then(|kind| kind.output(&from.pin))
            .is_some_and(|pin| pin.ty == PinType::Exec)
    }
}

#[cfg(test)]
#[path = "graph/tests.rs"]
mod tests;
