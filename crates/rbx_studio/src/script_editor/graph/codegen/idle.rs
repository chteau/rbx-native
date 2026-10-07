//! Statements left out of every run: legal, compiled to nothing, and worth
//! telling the user about.

use std::collections::BTreeSet;

use super::super::catalog::{self, Code, Kind};
use super::super::{Graph, NodeId};

/// Statements no event's run reaches. They compile to nothing, which is
/// right — Luau would never run them either — but worth saying.
pub(crate) fn idle(graph: &Graph) -> Vec<NodeId> {
    let mut reached = BTreeSet::new();
    let mut stack: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|node| catalog::kind(&node.kind).is_some_and(Kind::is_event))
        .map(|node| node.id)
        .collect();
    while let Some(at) = stack.pop() {
        for wire in graph.wires.iter().filter(|wire| wire.from.node == at) {
            if graph.is_exec(&wire.from) && reached.insert(wire.to.node) {
                stack.push(wire.to.node);
            }
        }
    }
    graph
        .nodes
        .iter()
        .filter(|node| {
            catalog::kind(&node.kind).is_some_and(|kind| {
                matches!(
                    kind.code,
                    Code::Statement(_) | Code::Branch | Code::ForEach | Code::Repeat | Code::Raw
                )
            })
        })
        .map(|node| node.id)
        .filter(|id| !reached.contains(id))
        .collect()
}
