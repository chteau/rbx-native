//! Keeping the canvas steady while the graph under it is rebuilt from
//! `Source`: positions, groups and the selection are carried from the old
//! graph to the new one by each node's codegen anchor, so a node the edit
//! did not touch stays where the user left it. Also the order the keyboard
//! walks the nodes in.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::catalog::Pin;
use super::codegen::{anchors, items, Origins};
use super::layout::place_new;
use super::{Graph, NodeId};

/// What carrying moved over.
pub(crate) struct Carried {
    /// The old selection, as it is in the new graph.
    pub(crate) selection: BTreeSet<NodeId>,
}

/// Nodes that compile to nothing: no event's run reaches them and no
/// compiled node reads them. A rebuild from `Source` cannot bring them back,
/// so [`carry`] keeps them.
pub(crate) fn loose(graph: &Graph) -> BTreeSet<NodeId> {
    let mut live: BTreeSet<NodeId> = graph
        .nodes
        .iter()
        .filter(|node| graph.kind_of(node.id).is_some_and(|kind| kind.is_event()))
        .map(|node| node.id)
        .collect();
    loop {
        let before = live.len();
        for wire in &graph.wires {
            let exec = graph.is_exec(&wire.from);
            if exec && live.contains(&wire.from.node) {
                live.insert(wire.to.node);
            }
            if !exec && live.contains(&wire.to.node) {
                live.insert(wire.from.node);
            }
        }
        if live.len() == before {
            break;
        }
    }
    graph
        .nodes
        .iter()
        .map(|node| node.id)
        .filter(|id| !live.contains(id))
        .collect()
}

/// The order Tab and the arrows visit nodes in: each event or start node,
/// then what its run goes on to, depth first; whatever no run reaches (values,
/// loose nodes) last, top to bottom.
pub(crate) fn exec_order(graph: &Graph, origins: &Origins) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for root in items(graph, origins) {
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            if !seen.insert(at) {
                continue;
            }
            order.push(at);
            let mut next: Vec<NodeId> = graph
                .wires
                .iter()
                .filter(|wire| wire.from.node == at && graph.is_exec(&wire.from))
                .map(|wire| wire.to.node)
                .collect();
            next.reverse();
            stack.extend(next);
        }
    }
    let mut rest: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| !seen.contains(&node.id))
        .collect();
    rest.sort_by(|a, b| (a.y, a.x, a.id).partial_cmp(&(b.y, b.x, b.id)).unwrap());
    order.extend(rest.into_iter().map(|node| node.id));
    order
}

/// Moves `old`'s layout onto `new` (a fresh import of changed code):
/// nodes with the same anchor keep their place; an edited node, whose anchor
/// changed, takes the place of an unmatched old node of its kind when the
/// counts agree; loose old nodes are kept with the wires among them; the
/// rest are placed clear of what is there.
pub(crate) fn carry(old: &Graph, new: &mut Graph, selection: &BTreeSet<NodeId>) -> Carried {
    let old_anchors = anchors(old);
    let new_anchors = anchors(new);
    let by_anchor: HashMap<&str, NodeId> = new_anchors
        .iter()
        .map(|(id, anchor)| (anchor.as_str(), *id))
        .collect();
    let loose = loose(old);
    // Old id -> new id.
    let mut map: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for node in &old.nodes {
        let Some(&id) = old_anchors
            .get(&node.id)
            .and_then(|anchor| by_anchor.get(anchor.as_str()))
        else {
            continue;
        };
        if !map.values().any(|taken| *taken == id) {
            map.insert(node.id, id);
        }
    }
    // Edited nodes: same kind, same count on both sides.
    let mut kinds: BTreeMap<&str, (Vec<NodeId>, Vec<NodeId>)> = BTreeMap::new();
    for node in old.nodes.iter().filter(|n| !loose.contains(&n.id)) {
        if !map.contains_key(&node.id) {
            kinds.entry(&node.kind).or_default().0.push(node.id);
        }
    }
    let taken: BTreeSet<NodeId> = map.values().copied().collect();
    for node in new.nodes.iter().filter(|n| !taken.contains(&n.id)) {
        kinds.entry(&node.kind).or_default().1.push(node.id);
    }
    for (_, (mut from, mut to)) in kinds {
        if from.len() == to.len() {
            from.sort();
            to.sort();
            map.extend(from.into_iter().zip(to));
        }
    }
    let mut placed = BTreeSet::new();
    for (&from, &to) in &map {
        if let (Some(at), Some(node)) = (old.node(from).map(|n| (n.x, n.y)), new.node_mut(to)) {
            (node.x, node.y) = at;
            placed.insert(to);
        }
    }
    // Loose nodes come along, renumbered after the new graph's own.
    let first = new.nodes.iter().map(|n| n.id + 1).max().unwrap_or(1);
    for (next, node) in (first..).zip(old.nodes.iter().filter(|n| loose.contains(&n.id))) {
        let mut copy = node.clone();
        copy.id = next;
        map.insert(node.id, next);
        placed.insert(next);
        new.nodes.push(copy);
    }
    for wire in &old.wires {
        if !loose.contains(&wire.from.node) && !loose.contains(&wire.to.node) {
            continue;
        }
        if let (Some(&from), Some(&to)) = (map.get(&wire.from.node), map.get(&wire.to.node)) {
            let mut wire = wire.clone();
            wire.from.node = from;
            wire.to.node = to;
            new.wires.push(wire);
        }
    }
    new.groups = old.groups.clone();
    place_new(new, &placed);
    Carried {
        selection: selection
            .iter()
            .filter_map(|id| map.get(id).copied())
            .collect(),
    }
}

/// Whether `id`'s first repeating group can take one more instance (`true`)
/// or give one up (`false`).
pub(crate) fn can_resize(graph: &Graph, id: NodeId, grow: bool) -> bool {
    let (Some(node), Some(kind)) = (graph.node(id), graph.kind_of(id)) else {
        return false;
    };
    kind.repeats
        .first()
        .is_some_and(|repeat| grow || repeat.count_in(&node.values) > repeat.min)
}

/// One more or one fewer instance of `id`'s first repeating pin group, as
/// "Add input" and "Remove input" ask. A pin that goes takes its wires and
/// its typed value with it. False when nothing changed.
pub(crate) fn resize_repeat(graph: &mut Graph, id: NodeId, grow: bool) -> bool {
    if !can_resize(graph, id, grow) {
        return false;
    }
    let Some(repeat) = graph.kind_of(id).and_then(|kind| kind.repeats.first()) else {
        return false;
    };
    let before = graph.pins(id);
    let Some(node) = graph.node_mut(id) else {
        return false;
    };
    let count = repeat.count_in(&node.values);
    let count = if grow { count + 1 } else { count - 1 };
    node.values
        .insert(repeat.count.to_owned(), count.to_string());
    let after = graph.pins(id);
    let gone = |before: &[Pin], after: &[Pin]| -> Vec<String> {
        before
            .iter()
            .filter(|pin| !after.iter().any(|kept| kept.name == pin.name))
            .map(|pin| pin.name.to_string())
            .collect()
    };
    let gone_in = gone(&before.inputs, &after.inputs);
    let gone_out = gone(&before.outputs, &after.outputs);
    graph.wires.retain(|wire| {
        !(wire.to.node == id && gone_in.contains(&wire.to.pin)
            || wire.from.node == id && gone_out.contains(&wire.from.pin))
    });
    if let Some(node) = graph.node_mut(id) {
        node.values.retain(|name, _| !gone_in.contains(name));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::catalog;
    use super::super::codegen::compile_with;
    use super::super::import::import;
    use super::super::End;
    use super::*;

    const SOURCE: &str = "local Players = game:GetService(\"Players\")\n\nPlayers.PlayerAdded:Connect(function(player)\n\tprint(\"hello\")\n\tprint(\"there\")\nend)\n\nprint(\"done\")\n";

    #[test]
    fn opening_and_tidying_leave_the_source_alone() {
        let imported = import(SOURCE);
        assert!(imported.broken.is_none());
        assert_eq!(
            compile_with(&imported.graph, &imported.origins).as_deref(),
            Ok(SOURCE)
        );
        // Moving every node is a layout change, never a code change.
        let mut graph = imported.graph.clone();
        crate::script_editor::graph::layout::tidy(&mut graph);
        assert_eq!(
            compile_with(&graph, &imported.origins).as_deref(),
            Ok(SOURCE)
        );
    }

    #[test]
    fn editing_one_statement_keeps_the_others_where_they_were() {
        let imported = import(SOURCE);
        let mut graph = imported.graph;
        for (i, node) in graph.nodes.iter_mut().enumerate() {
            node.x = 1000.0 + 300.0 * i as f32;
            node.y = 77.0 + i as f32;
        }
        let at: BTreeMap<NodeId, (f32, f32)> =
            graph.nodes.iter().map(|n| (n.id, (n.x, n.y))).collect();
        let edited = graph
            .nodes
            .iter()
            .find(|n| n.values.values().any(|v| v.contains("there")))
            .map(|n| n.id)
            .expect("the statement to edit");
        let node = graph.node_mut(edited).unwrap();
        for value in node.values.values_mut() {
            *value = value.replace("there", "again");
        }
        let code = compile_with(&graph, &imported.origins).unwrap();
        assert!(code.contains("again") && !code.contains("there"), "{code}");

        let mut rebuilt = import(&code).graph;
        let selection = BTreeSet::from([edited]);
        let carried = carry(&graph, &mut rebuilt, &selection);
        assert_eq!(rebuilt.nodes.len(), graph.nodes.len());
        let moved = rebuilt
            .nodes
            .iter()
            .filter(|n| at.values().all(|&p| p != (n.x, n.y)))
            .count();
        assert_eq!(moved, 0, "every node, edited one included, kept its place");
        assert_eq!(carried.selection.len(), 1);
    }

    #[test]
    fn the_keyboard_walks_nodes_in_run_order() {
        let imported = import(SOURCE);
        let order = exec_order(&imported.graph, &imported.origins);
        assert_eq!(order.len(), imported.graph.nodes.len());
        let first = imported.graph.kind_of(order[0]).unwrap();
        assert!(first.is_event(), "an event or start node leads");
    }

    #[test]
    fn a_repeating_pin_can_be_added_and_removed() {
        let mut graph = Graph::default();
        let call = catalog::kind("call").expect("a call node");
        let id = graph.add(call, [0.0, 0.0]);
        // `add` starts a minimum-zero repeat at one instance.
        assert!(graph.input_pin(id, "Input 1").is_some());
        assert!(resize_repeat(&mut graph, id, true));
        assert!(graph.input_pin(id, "Input 2").is_some());
        graph.set_value(&End::new(id, "Input 2"), "5".into());
        assert!(resize_repeat(&mut graph, id, false));
        assert!(graph.input_pin(id, "Input 2").is_none());
        assert!(!graph.node(id).unwrap().values.contains_key("Input 2"));
        assert!(resize_repeat(&mut graph, id, false));
        assert!(!can_resize(&graph, id, false), "none left to remove");
    }

    #[test]
    fn a_node_nothing_wires_up_survives_a_rebuild() {
        let imported = import(SOURCE);
        let mut graph = imported.graph;
        let print = catalog::all()
            .find(|k| k.key == "print")
            .expect("a print node");
        let loose_id = graph.add(print, [-500.0, -500.0]);
        assert!(loose(&graph).contains(&loose_id));
        let mut rebuilt = import(SOURCE).graph;
        carry(&graph, &mut rebuilt, &BTreeSet::new());
        assert_eq!(rebuilt.nodes.len(), graph.nodes.len());
        assert!(rebuilt.nodes.iter().any(|n| (n.x, n.y) == (-500.0, -500.0)));
    }
}
