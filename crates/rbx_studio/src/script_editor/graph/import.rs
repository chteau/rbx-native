//! Luau read back into a graph. Every construct becomes generic syntax
//! nodes; a catalog node stands in only where it writes the same tokens.
//! Each piece is checked against its source, and one that would come out
//! different is kept as written in a Luau Code node.

mod emit;
mod pattern;
mod tree;

#[cfg(test)]
mod corpus;
#[cfg(test)]
#[path = "import/tests.rs"]
mod tests;

use std::collections::HashSet;

use super::catalog::{self, PinType};
use super::codegen::Origins;
use super::{End, Graph, NodeId};

/// A script read back: its graph, where its statements came from, and the
/// syntax error when it did not parse (the graph then holds it whole).
pub(crate) struct Imported {
    pub(crate) graph: Graph,
    pub(crate) origins: Origins,
    pub(crate) broken: Option<Broken>,
}

pub(crate) struct Broken {
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// Token and comment spans of `code`, or none when it does not parse.
fn spans(code: &str) -> Option<tree::Tree> {
    tree::snippet(code)
}

/// The names a piece of Luau reads or writes, so generated locals avoid
/// them. Anything word-like when it does not parse.
pub(crate) fn names(code: &str) -> Vec<String> {
    fn words(text: &str) -> impl Iterator<Item = String> + '_ {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
            .map(str::to_owned)
    }
    match spans(code) {
        Some(t) => t
            .tokens
            .iter()
            .map(|&(a, b)| &code[a..b])
            .filter(|text| text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
            .map(str::to_owned)
            .collect(),
        None => words(code).collect(),
    }
}

/// A Luau Code node's lines, each marked when it lies inside a multi-line
/// string or comment and so must be written exactly as it is, unindented.
pub(crate) fn raw_lines(code: &str) -> Vec<(&str, bool)> {
    if code.is_empty() {
        return Vec::new();
    }
    let spans: Option<Vec<(usize, usize)>> = spans(code).map(|t| {
        t.tokens
            .iter()
            .copied()
            .chain(t.comments.iter().map(|c| (c.lo, c.hi)))
            .filter(|&(a, b)| code[a..b].contains('\n'))
            .collect()
    });
    let mut start = 0;
    code.split('\n')
        .map(|line| {
            let verbatim = spans
                .as_ref()
                .is_none_or(|spans| spans.iter().any(|&(a, b)| a < start && start < b));
            start += line.len() + 1;
            match verbatim {
                true => (line, true),
                false => (line.trim_end_matches('\r'), false),
            }
        })
        .collect()
}

/// Whether `b` is `a` again: the same tokens and the same comments.
pub(crate) fn same(a: &str, b: &str) -> bool {
    let (Some(x), Some(y)) = (spans(a), spans(b)) else {
        return a == b;
    };
    let toks = |t: &tree::Tree, s: &str| -> Vec<String> {
        // A `;` between statements or fields is not code; the origin keeps it.
        t.tokens
            .iter()
            .map(|&(lo, hi)| &s[lo..hi])
            .filter(|text| *text != ";")
            .map(str::to_owned)
            .collect()
    };
    let notes = |t: &tree::Tree, s: &str| -> Vec<String> {
        t.comments
            .iter()
            .map(|c| s[c.lo..c.hi].trim_end().to_owned())
            .collect()
    };
    toks(&x, a) == toks(&y, b) && notes(&x, a) == notes(&y, b)
}

/// `source` as a graph that compiles back to the same code.
pub(crate) fn import(source: &str) -> Imported {
    if source.trim().is_empty() {
        return Imported {
            graph: Graph::default(),
            origins: Origins::default(),
            broken: None,
        };
    }
    let ast = match full_moon::parse(source) {
        Ok(ast) => ast,
        Err(errors) => {
            let (line, message) = errors.first().map_or((1, String::new()), |e| {
                (e.range().0.line(), e.error_message().to_string())
            });
            let (graph, origins) = whole(source);
            return Imported {
                graph,
                origins,
                broken: Some(Broken { line, message }),
            };
        }
    };
    let tree = tree::tree(source, &ast);
    let (mut graph, origins) = emit::build(source, &tree);
    layout(&mut graph, &origins.order);
    Imported {
        graph,
        origins,
        broken: None,
    }
}

/// The whole text as one Luau Code node, run at the start.
fn whole(source: &str) -> (Graph, Origins) {
    let mut graph = Graph::default();
    let mut origins = Origins::default();
    let (Some(start), Some(luau)) = (catalog::kind("start"), catalog::kind("luau")) else {
        return (graph, origins);
    };
    let start = graph.add(start, [0.0, 0.0]);
    let raw = graph.add(luau, [0.0, 0.0]);
    graph.set_value(
        &End::new(raw, "Code"),
        source.trim_end_matches(['\r', '\n']).to_owned(),
    );
    let _ = graph.connect(End::new(start, ""), End::new(raw, ""));
    layout(&mut graph, &[start]);
    origins.order.push(start);
    (graph, origins)
}

/// Each event (or run of top-level code) in a row of its own, its run
/// left to right, values hung below the node that reads them.
fn layout(graph: &mut Graph, items: &[NodeId]) {
    let mut placed = HashSet::new();
    let mut y = 0.0;
    for &item in items {
        y = place(graph, item, 0.0, y, &mut placed) + 80.0;
    }
}

/// Places `node` and all it leads to; the lowest edge drawn.
fn place(graph: &mut Graph, node: NodeId, x: f32, y: f32, placed: &mut HashSet<NodeId>) -> f32 {
    if !placed.insert(node) {
        return y;
    }
    if let Some(at) = graph.node_mut(node) {
        (at.x, at.y) = (x, y);
    }
    let rect = graph.node(node).map(|at| super::layout::rect(graph, at));
    let (w, h) = rect.map_or((200.0, 60.0), |rect| (rect.w, rect.h));
    let pins = graph.pins(node);
    let targets = |graph: &Graph, names: &[&str]| -> Vec<NodeId> {
        names
            .iter()
            .flat_map(|pin| {
                graph
                    .wires_from(&End::new(node, pin))
                    .map(|wire| wire.to.node)
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let mut bottom = y + h;
    for next in targets(graph, &["", "Completed"]) {
        bottom = bottom.max(place(graph, next, x + w + 60.0, y, placed));
    }
    let values: Vec<NodeId> = pins
        .inputs
        .iter()
        .filter(|pin| pin.ty != PinType::Exec)
        .filter_map(|pin| {
            graph
                .wire_into(&End::new(node, pin.name))
                .map(|wire| wire.from.node)
        })
        .collect();
    let mut below = y + h;
    for value in values {
        below = place(graph, value, x + 20.0, below + 20.0, placed);
    }
    bottom = bottom.max(below);
    let inner: Vec<&str> = pins
        .outputs
        .iter()
        .filter(|pin| pin.ty == PinType::Exec && !matches!(pin.name, "" | "Completed"))
        .map(|pin| pin.name)
        .collect();
    for next in targets(graph, &inner) {
        bottom = place(graph, next, x + 40.0, bottom + 20.0, placed);
    }
    bottom
}
