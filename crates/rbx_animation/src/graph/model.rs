//! The authored half of an Animation Graph: nodes, the pins wiring them,
//! their properties, state-machine transitions, the graph's parameters, and
//! the masks and expression nodes it shares between nodes.

use std::collections::BTreeMap;

pub mod enums;
mod kind;
mod node;
mod param;
mod shared;
mod transition;

pub use kind::{NodeKind, DEFAULT_TRANSITION_DURATION};
pub use node::{Input, Node};
pub(crate) use param::unique;
pub use param::{sanitize_name, Param, ParamKind, ParamRef};
pub use shared::{Expression, Mask, MaskJoint};
pub(crate) use transition::ENGINE_TRANSITION_DURATION;
pub use transition::{Transition, WaitFor, When};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl Value {
    pub fn number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Text(t) => t.trim().parse().unwrap_or(0.),
        }
    }

    /// Luau truthiness: everything but `false` (and nil, which has no value
    /// here) is true.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Bool(false))
    }

    pub fn text(&self) -> String {
        match self {
            Value::Number(n) => format!("{n}"),
            Value::Bool(b) => b.to_string(),
            Value::Text(t) => t.clone(),
        }
    }
}

/// A node property: a fixed value, bound to a graph parameter, or wired to
/// an expression node with this source.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Value(Value),
    Param(String),
    Expr(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub name: String,
    pub nodes: Vec<Node>,
    pub params: Vec<Param>,
    /// The parameter pills drawn on the canvas. Storage needs at least one
    /// per parameter (it is where the type lives) and adds it when missing.
    pub param_refs: Vec<ParamRef>,
    pub expressions: Vec<Expression>,
    pub masks: Vec<Mask>,
    /// The definition's `RBX_` attributes (`RBX_SourceAssetId`,
    /// `RBX_GraphDirty`, `RBX_GraphVersion`), which are editor bookkeeping
    /// and never parameters. Kept as read.
    pub rbx_attributes: BTreeMap<String, Value>,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            name: "AnimationGraph".into(),
            nodes: vec![Node::new(NodeKind::GraphOutput, "GraphOutput")],
            params: Vec::new(),
            param_refs: Vec::new(),
            expressions: Vec::new(),
            masks: Vec::new(),
            rbx_attributes: BTreeMap::new(),
        }
    }
}

impl Graph {
    pub fn output(&self) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.kind == NodeKind::GraphOutput)
    }

    /// Adds a node named as the editor names it (`ClipNode`, `ClipNode1`, …)
    /// and returns its index.
    pub fn add(&mut self, kind: NodeKind) -> usize {
        let id = unique(kind.type_name(), |id| self.nodes.iter().any(|n| n.id == id));
        self.nodes.push(Node::new(kind, &id));
        self.nodes.len() - 1
    }

    /// Wires `from`'s output into `to`'s `pin`. On a node with a list of
    /// inputs a pin that does not exist yet is made. Refused when it would
    /// make a cycle, which no node could evaluate.
    pub fn connect(&mut self, from: usize, to: usize, pin: &str) -> bool {
        if from >= self.nodes.len() || to >= self.nodes.len() || self.reaches(from, to) {
            return false;
        }
        let node = &mut self.nodes[to];
        if !node.inputs.iter().any(|i| i.pin == pin) {
            if !node.kind.grows_pins() || pin.is_empty() {
                return false;
            }
            node.push_pin(pin);
        }
        if let Some(input) = node.inputs.iter_mut().find(|i| i.pin == pin) {
            input.node = Some(from);
        }
        true
    }

    /// Wires `from` into a new pin of `to`, named after `from` as the editor
    /// does when a wire is dropped on the placeholder row ("Clip", "Clip1",
    /// …). Returns the pin's name.
    pub fn connect_new(&mut self, from: usize, to: usize) -> Option<String> {
        let source = self.nodes.get(from)?;
        let target = self.nodes.get(to).filter(|n| n.kind.grows_pins())?;
        let pin = target.unique_pin(&source.pin_stem());
        self.connect(from, to, &pin).then_some(pin)
    }

    pub fn disconnect(&mut self, to: usize, pin: &str) {
        if let Some(input) = self
            .nodes
            .get_mut(to)
            .and_then(|n| n.inputs.iter_mut().find(|i| i.pin == pin))
        {
            input.node = None;
        }
    }

    /// Whether `to` feeds (directly or not) into `from`, or is `from`.
    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut stack = vec![from];
        let mut seen = vec![false; self.nodes.len()];
        while let Some(at) = stack.pop() {
            if at == to {
                return true;
            }
            if std::mem::replace(&mut seen[at], true) {
                continue;
            }
            stack.extend(self.nodes[at].inputs.iter().filter_map(|i| i.node));
        }
        false
    }

    /// Removes a node, unwiring everything that pointed at it. The output
    /// node stays.
    pub fn remove(&mut self, index: usize) -> bool {
        if self
            .nodes
            .get(index)
            .is_none_or(|n| n.kind == NodeKind::GraphOutput)
        {
            return false;
        }
        self.nodes.remove(index);
        for node in &mut self.nodes {
            for input in &mut node.inputs {
                input.node = match input.node {
                    Some(i) if i == index => None,
                    Some(i) if i > index => Some(i - 1),
                    other => other,
                };
            }
        }
        true
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Adds a parameter named as the editor names a new one
    /// (`NumberParameter`, `NumberParameter1`, …) with its type's default,
    /// and returns its name.
    pub fn add_param(&mut self, kind: ParamKind) -> String {
        let base = format!("{}Parameter", kind.type_name());
        let name = unique(&base, |n| self.param(n).is_some());
        self.params.push(Param {
            name: name.clone(),
            kind,
            default: kind.default_value(),
        });
        name
    }

    pub fn mask(&self, name: &str) -> Option<&Mask> {
        self.masks.iter().find(|m| m.name == name)
    }
}

#[cfg(test)]
mod tests;
