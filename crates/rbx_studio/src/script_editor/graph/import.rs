//! Luau read back into a graph. Statements a catalog node writes are drawn
//! as that node; everything else stays as written, in Luau Code nodes, so
//! an import never loses code. The result is checked: it must compile to
//! the same tokens and comments as the source (locals may be renamed), and
//! any statement that would come out different is kept as written instead.

mod build;
mod lex;
mod parse;

use std::collections::{HashMap, HashSet};

use super::catalog;
use super::codegen;
use super::{End, Graph, NodeId};
use lex::{Lexed, T};

/// The names a piece of Luau reads or writes, so generated locals avoid
/// them. Anything word-like when the text does not lex.
pub(crate) fn names(code: &str) -> Vec<String> {
    fn words(text: &str) -> impl Iterator<Item = String> + '_ {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
            .map(str::to_owned)
    }
    match lex::lex(code) {
        Some(lexed) => lexed
            .toks
            .iter()
            .filter(|tok| matches!(tok.t, T::Name | T::Interp))
            .flat_map(|tok| words(&code[tok.start..tok.end]).collect::<Vec<_>>())
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
    let spans: Option<Vec<(usize, usize)>> = lex::lex(code).map(|lexed| {
        lexed
            .toks
            .iter()
            .map(|tok| (tok.start, tok.end))
            .chain(lexed.comments.iter().copied())
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

/// Whether `b` is `a` again, as far as the importer's check goes: the
/// same tokens and comments, locals renamed consistently.
pub(crate) fn same(a: &str, b: &str) -> bool {
    match lex::lex(a) {
        Some(lexed) => verify(&lexed, a, b).is_ok(),
        None => a == b,
    }
}

/// `source` as a graph that compiles back to the same code.
pub(crate) fn import(source: &str) -> Graph {
    if source.trim().is_empty() {
        return Graph::default();
    }
    let Some(lexed) = lex::lex(source) else {
        return whole(source);
    };
    let p = parse::P {
        src: source,
        toks: &lexed.toks,
    };
    let Some((top, _)) = p.block(0, &[], 0) else {
        return whole(source);
    };
    let mut pinned = HashSet::new();
    // ponytail: rebuilds the whole graph per statement kept as written;
    // fine for scripts of a few hundred lines.
    loop {
        let mut builder = build::Builder::new(&p, &lexed.comments, &pinned);
        builder.top(&top.stmts, top.lo, top.hi);
        let pin = match codegen::compile(&builder.graph) {
            Err(problems) => problems
                .first()
                .and_then(|problem| builder.owner.get(&problem.node).copied()),
            Ok(out) => match verify(&lexed, source, &out) {
                Ok(()) => {
                    let mut graph = builder.graph;
                    layout(&mut graph, &builder.items);
                    return graph;
                }
                Err(Some(n)) => culprit(&builder.converted, n),
                Err(None) => None,
            },
        };
        match pin {
            Some(key) if pinned.insert(key) => {}
            _ => return whole(source),
        }
    }
}

/// The statement to keep as written for a difference at source token `n`:
/// the innermost converted one holding it, or the last one before it.
fn culprit(converted: &[(usize, usize)], n: usize) -> Option<usize> {
    converted
        .iter()
        .filter(|&&(lo, hi)| lo <= n && n < hi)
        .min_by_key(|&&(lo, hi)| hi - lo)
        .or_else(|| {
            converted
                .iter()
                .filter(|&&(_, hi)| hi <= n)
                .max_by_key(|&&(_, hi)| hi)
        })
        .map(|&(lo, _)| lo)
}

/// The whole text as one Luau Code node, run at the start.
fn whole(source: &str) -> Graph {
    let mut graph = Graph::default();
    let (Some(start), Some(luau)) = (catalog::kind("start"), catalog::kind("luau")) else {
        return graph;
    };
    let start = graph.add(start, [0.0, 0.0]);
    let raw = graph.add(luau, [0.0, 0.0]);
    graph.set_value(
        &End::new(raw, "Code"),
        source.trim_end_matches(['\r', '\n']).to_owned(),
    );
    let _ = graph.connect(End::new(start, ""), End::new(raw, ""));
    layout(&mut graph, &[start]);
    graph
}

/// Whether `out` is the source again: the same comments, and the same
/// tokens but for locals and parameters renamed consistently. On a
/// difference, the source token it shows at, where there is one.
fn verify(src: &Lexed, source: &str, out: &str) -> Result<(), Option<usize>> {
    let made = lex::lex(out).ok_or(None)?;
    let comments = |lexed: &Lexed, text: &str| -> Vec<String> {
        lexed
            .comments
            .iter()
            .map(|&(a, b)| text[a..b].trim_end().to_owned())
            .collect()
    };
    if comments(src, source) != comments(&made, out) {
        return Err(None);
    }
    let slice = |text: &str, tok: &lex::Tok| -> String { text[tok.start..tok.end].to_owned() };
    let mut fwd: HashMap<String, (String, usize)> = HashMap::new();
    let mut back: HashMap<String, String> = HashMap::new();
    let (mut binding, mut pending_fn, mut params) = (false, false, false);
    for n in 0..src.toks.len().max(made.toks.len()) {
        let (Some(s), Some(o)) = (src.toks.get(n), made.toks.get(n)) else {
            return Err(Some(n.min(src.toks.len().saturating_sub(1))));
        };
        if s.t != o.t {
            return Err(Some(n));
        }
        let (st, ot) = (slice(source, s), slice(out, o));
        match s.t {
            T::Name => {
                if binding || params {
                    if back.get(&ot).is_some_and(|was| *was != st) {
                        return Err(Some(n));
                    }
                    back.insert(ot.clone(), st.clone());
                    fwd.insert(st, (ot, n));
                    continue;
                }
                match fwd.get(&st) {
                    Some((mapped, site)) if *mapped != ot => return Err(Some(*site)),
                    Some(_) => {}
                    None if st != ot => return Err(Some(n)),
                    None => {}
                }
                if back.get(&ot).is_some_and(|was| *was != st) {
                    return Err(Some(n));
                }
            }
            T::Str => {
                if st != ot
                    && (lex::unquote(&st).is_none() || lex::unquote(&st) != lex::unquote(&ot))
                {
                    return Err(Some(n));
                }
            }
            _ if st != ot => return Err(Some(n)),
            _ => {}
        }
        match st.as_str() {
            "local" | "for" if s.t == T::Keyword => binding = true,
            "," | ":" if binding => {}
            "function" if s.t == T::Keyword => {
                binding = false;
                pending_fn = true;
            }
            "(" if pending_fn => {
                pending_fn = false;
                params = true;
            }
            ")" if params => params = false,
            _ if s.t != T::Name => binding = false,
            _ => {}
        }
    }
    Ok(())
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
    let Some(kind) = graph.kind_of(node) else {
        return y;
    };
    if let Some(at) = graph.node_mut(node) {
        (at.x, at.y) = (x, y);
    }
    let rect = graph.node(node).map(|at| super::layout::rect(graph, at));
    let (w, h) = rect.map_or((200.0, 60.0), |rect| (rect.w, rect.h));
    let targets = |graph: &Graph, pins: &[&str]| -> Vec<NodeId> {
        pins.iter()
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
    let values: Vec<NodeId> = kind
        .inputs
        .iter()
        .filter(|pin| pin.ty != catalog::PinType::Exec)
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
    for inner in targets(graph, &["True", "False", "Loop"]) {
        bottom = place(graph, inner, x + 40.0, bottom + 20.0, placed);
    }
    bottom
}

#[cfg(test)]
#[path = "import/tests.rs"]
mod tests;
