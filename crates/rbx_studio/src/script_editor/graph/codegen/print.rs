//! What identifies a statement across edits: a hash of its content (so an
//! untouched one can be spliced back verbatim) and a text key per node (so
//! a node of a rewritten graph can be matched to the one it came from).

use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};

use super::super::catalog::{Code, PinType};
use super::{continuation, is_value, items, Compiler, Origins, Scope};
use super::{End, Graph, NodeId};

/// A hash of one statement: its kind, its typed values, the values wired
/// into it and the runs nested in it. Not its id or position, and not what
/// follows it, so moving a node or editing its neighbour leaves it alone.
pub(crate) fn fingerprint(graph: &Graph, node: NodeId) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_node(graph, node, &mut hasher, &mut Vec::new());
    hasher.finish()
}

fn hash_node(graph: &Graph, id: NodeId, h: &mut impl Hasher, open: &mut Vec<NodeId>) {
    let (Some(node), Some(kind)) = (graph.node(id), graph.kind_of(id)) else {
        return;
    };
    // A loop in the graph is a problem for the compiler to report.
    if open.contains(&id) {
        return;
    }
    open.push(id);
    node.kind.hash(h);
    node.values.hash(h);
    let pins = graph.pins(id);
    for pin in pins.inputs.iter().filter(|pin| pin.ty != PinType::Exec) {
        match graph.wire_into(&End::new(id, pin.name)) {
            Some(wire) => hash_source(graph, &wire.from, h, open),
            None => 0u8.hash(h),
        }
    }
    let carry_on = continuation(kind);
    for pin in pins.outputs.iter().filter(|pin| pin.ty == PinType::Exec) {
        if carry_on == Some(pin.name) {
            continue;
        }
        pin.name.hash(h);
        let mut at = End::new(id, pin.name);
        loop {
            let next = graph.wires_from(&at).next().map(|wire| wire.to.node);
            let Some(next) = next else { break };
            if open.contains(&next) {
                break;
            }
            hash_node(graph, next, h, open);
            match graph.kind_of(next).and_then(continuation) {
                Some(pin) => at = End::new(next, pin),
                None => break,
            }
        }
    }
    open.pop();
}

fn hash_source(graph: &Graph, from: &End, h: &mut impl Hasher, open: &mut Vec<NodeId>) {
    match graph.kind_of(from.node) {
        Some(kind) if is_value(kind) => {
            1u8.hash(h);
            from.pin.hash(h);
            hash_node(graph, from.node, h, open);
        }
        // An event's parameter or a loop's variable: where it is bound is
        // the statement's business only through its name.
        Some(kind) => {
            2u8.hash(h);
            kind.key.hash(h);
            from.pin.hash(h);
        }
        None => 3u8.hash(h),
    }
}

/// A key for every node: its kind and the code it writes on its own, with
/// the nth identical one numbered n. The same script imported twice, or
/// edited elsewhere, gives the same keys to the same nodes.
#[allow(dead_code)] // the importer matches nodes by it
pub(crate) fn anchors(graph: &Graph) -> HashMap<NodeId, String> {
    let origins = Origins::default();
    let mut compiler = Compiler::new(graph, &origins);
    compiler.elide = true;
    let mut seen = BTreeSet::new();
    let mut walk = Vec::new();
    for id in items(graph, &origins) {
        visit(graph, id, &mut seen, &mut walk);
    }
    let mut rest: Vec<_> = graph.nodes.iter().collect();
    rest.sort_by(|a, b| (a.y, a.x, a.id).partial_cmp(&(b.y, b.x, b.id)).unwrap());
    for node in rest {
        visit(graph, node.id, &mut seen, &mut walk);
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut keys = HashMap::new();
    for id in walk {
        let Some(kind) = graph.kind_of(id) else {
            continue;
        };
        let text = own_code(&mut compiler, id);
        let base = format!("{}|{text}", kind.key);
        let n = counts.entry(base.clone()).or_default();
        keys.insert(id, format!("{base}#{n}"));
        *n += 1;
    }
    keys
}

/// The code a node writes with no runs and no locals, on one line.
fn own_code(compiler: &mut Compiler, id: NodeId) -> String {
    compiler.out.clear();
    compiler.indent = 0;
    compiler.lead = None;
    compiler.problems.clear();
    let mut scope = Scope::new();
    let Some(kind) = compiler.graph.kind_of(id) else {
        return String::new();
    };
    let text = match kind.code {
        Code::Event(signal) => signal
            .and_then(|signal| compiler.fill(id, signal, super::Prec::Atom, &mut scope))
            .unwrap_or_default(),
        _ if is_value(kind) => {
            let pin = kind.outputs.iter().find(|pin| pin.ty != PinType::Exec);
            pin.and_then(|pin| compiler.value(&End::new(id, pin.name), &mut scope))
                .map(|(text, _)| text)
                .unwrap_or_default()
        }
        _ => {
            compiler.statement(id, &mut scope);
            std::mem::take(&mut compiler.out)
        }
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Depth first: a node, the values it reads, then the runs under it.
fn visit(graph: &Graph, id: NodeId, seen: &mut BTreeSet<NodeId>, walk: &mut Vec<NodeId>) {
    let Some(kind) = graph.kind_of(id) else {
        return;
    };
    if !seen.insert(id) {
        return;
    }
    walk.push(id);
    let pins = graph.pins(id);
    for pin in pins.inputs.iter().filter(|pin| pin.ty != PinType::Exec) {
        if let Some(wire) = graph.wire_into(&End::new(id, pin.name)) {
            if graph.kind_of(wire.from.node).is_some_and(is_value) {
                visit(graph, wire.from.node, seen, walk);
            }
        }
    }
    let carry_on = continuation(kind);
    for pin in pins.outputs.iter().filter(|pin| pin.ty == PinType::Exec) {
        if carry_on != Some(pin.name) {
            visit_run(graph, End::new(id, pin.name), seen, walk);
        }
    }
}

fn visit_run(graph: &Graph, mut at: End, seen: &mut BTreeSet<NodeId>, walk: &mut Vec<NodeId>) {
    loop {
        let next = graph.wires_from(&at).next().map(|wire| wire.to.node);
        let Some(next) = next else { break };
        if seen.contains(&next) {
            break;
        }
        visit(graph, next, seen, walk);
        match graph.kind_of(next).and_then(continuation) {
            Some(pin) => at = End::new(next, pin),
            None => break,
        }
    }
}
