//! Graph to Luau. Each event becomes a connection (or, for On Start, the
//! top level itself) whose body follows the run-order wires; values are
//! written inline where they are used, except a value read more than once
//! or one that does work (a lookup, a clone), which is read into a local
//! just before the first statement that needs it.
//!
//! A graph that cannot compile — an input left unwired that has no
//! literal, a run that loops into itself, a value used outside the run
//! that gives it — gives back every problem instead, each pinned to its
//! node, and nothing is written.

use std::collections::{BTreeMap, BTreeSet};

use super::catalog::{self, Code, Kind, PinType, Prec};
use super::{End, Graph, NodeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) node: NodeId,
    pub(crate) message: String,
}

pub(crate) fn compile(graph: &Graph) -> Result<String, Vec<Problem>> {
    let mut compiler = Compiler {
        graph,
        lines: Vec::new(),
        problems: Vec::new(),
        names: BTreeSet::new(),
        running: Vec::new(),
        resolving: Vec::new(),
    };
    for node in &graph.nodes {
        if catalog::kind(&node.kind).is_none() {
            compiler.problem(node.id, format!("Unknown node \"{}\"", node.kind));
        }
    }
    let mut events: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| catalog::kind(&node.kind).is_some_and(Kind::is_event))
        .collect();
    events.sort_by(|a, b| (a.y, a.x, a.id).partial_cmp(&(b.y, b.x, b.id)).unwrap());
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            compiler.lines.push(String::new());
        }
        compiler.event(event.id);
    }
    match compiler.problems.is_empty() {
        true => Ok(compiler.finish()),
        false => Err(compiler.problems),
    }
}

/// Which variable holds each value already read in a block.
type Scope = BTreeMap<End, String>;

struct Compiler<'g> {
    graph: &'g Graph,
    lines: Vec<String>,
    problems: Vec<Problem>,
    /// Every name given out, so no two locals or parameters share one.
    names: BTreeSet<String>,
    /// The statements of the run being written, to catch one looping back.
    running: Vec<NodeId>,
    /// The values being written, to catch a value loop a hand-edited graph
    /// could hold.
    resolving: Vec<NodeId>,
}

/// Names a fresh local must not take: Luau's keywords and the globals a
/// generated script reads.
const TAKEN: &[&str] = &[
    "and",
    "break",
    "do",
    "else",
    "elseif",
    "end",
    "false",
    "for",
    "function",
    "if",
    "in",
    "local",
    "nil",
    "not",
    "or",
    "repeat",
    "return",
    "then",
    "true",
    "until",
    "while",
    "continue",
    "type",
    "export",
    "game",
    "workspace",
    "script",
    "task",
    "math",
    "print",
    "warn",
    "tostring",
    "tonumber",
    "Instance",
    "Vector3",
    "Color3",
    "string",
    "table",
];

impl<'g> Compiler<'g> {
    fn finish(self) -> String {
        let mut text = self.lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }

    fn problem(&mut self, node: NodeId, message: String) {
        self.problems.push(Problem { node, message });
    }

    fn kind(&self, node: NodeId) -> &'static Kind {
        self.graph
            .kind_of(node)
            .expect("only nodes of known kinds are compiled")
    }

    fn emit(&mut self, indent: usize, line: String) {
        self.lines.push(format!("{}{line}", "\t".repeat(indent)));
    }

    fn event(&mut self, node: NodeId) {
        let kind = self.kind(node);
        let mut scope = Scope::new();
        let Code::Event(signal) = kind.code else {
            return;
        };
        let Some(signal) = signal else {
            self.run(&End::new(node, ""), &mut scope, 0);
            return;
        };
        let Some(signal) = self.fill(node, signal, Prec::Atom, &mut Scope::new()) else {
            return;
        };
        let params: Vec<String> = kind
            .outputs
            .iter()
            .filter(|pin| pin.ty != PinType::Exec)
            .map(|pin| {
                let name = self.fresh(pin.name);
                scope.insert(End::new(node, pin.name), name.clone());
                name
            })
            .collect();
        self.emit(
            0,
            format!("{signal}:Connect(function({})", params.join(", ")),
        );
        self.run(&End::new(node, ""), &mut scope, 1);
        self.emit(0, "end)".into());
    }

    /// Writes the run leaving `from`, one statement after another.
    fn run(&mut self, from: &End, scope: &mut Scope, indent: usize) {
        let Some(next) = self.graph.wires_from(from).next().map(|wire| wire.to.node) else {
            return;
        };
        if self.running.contains(&next) {
            self.problem(next, "This run loops back into itself".into());
            return;
        }
        self.running.push(next);
        self.statement(next, scope, indent);
        self.running.pop();
    }

    fn statement(&mut self, node: NodeId, scope: &mut Scope, indent: usize) {
        self.hoist(node, scope, indent);
        let exec = |pin: &str| End::new(node, pin);
        match self.kind(node).code {
            Code::Statement(template) => {
                if let Some(line) = self.fill(node, template, Prec::Atom, scope) {
                    self.emit(indent, line);
                }
                self.run(&exec(""), scope, indent);
            }
            Code::Branch => {
                let Some((condition, prec)) = self.input(node, "Condition", scope) else {
                    return;
                };
                let wired = |pin: &str| self.graph.wires_from(&exec(pin)).next().is_some();
                let (yes, no) = (wired("True"), wired("False"));
                if !yes && no {
                    let condition = wrap(condition, prec < Prec::Unary);
                    self.emit(indent, format!("if not {condition} then"));
                    self.block(&exec("False"), scope, indent);
                } else {
                    self.emit(indent, format!("if {condition} then"));
                    self.block(&exec("True"), scope, indent);
                    if no {
                        self.emit(indent, "else".into());
                        self.block(&exec("False"), scope, indent);
                    }
                }
                self.emit(indent, "end".into());
            }
            Code::ForEach => {
                let Some((list, _)) = self.input(node, "List", scope) else {
                    return;
                };
                let mut inner = scope.clone();
                let index = self.bind(node, "Index", &mut inner);
                let item = self.bind(node, "Item", &mut inner);
                self.emit(indent, format!("for {index}, {item} in {list} do"));
                self.looped(node, inner, scope, indent);
            }
            Code::Repeat => {
                let Some((count, _)) = self.input(node, "Count", scope) else {
                    return;
                };
                let mut inner = scope.clone();
                let index = self.bind(node, "Index", &mut inner);
                self.emit(indent, format!("for {index} = 1, {count} do"));
                self.looped(node, inner, scope, indent);
            }
            Code::Event(_) | Code::Expression { .. } => {}
        }
    }

    /// A loop's own output, named for the loop's body alone.
    fn bind(&mut self, node: NodeId, pin: &str, inner: &mut Scope) -> String {
        let name = self.fresh(pin);
        inner.insert(End::new(node, pin), name.clone());
        name
    }

    /// The loop's body, its `end`, then whatever runs once it completes.
    fn looped(&mut self, node: NodeId, mut inner: Scope, scope: &mut Scope, indent: usize) {
        self.run(&End::new(node, "Loop"), &mut inner, indent + 1);
        self.emit(indent, "end".into());
        self.run(&End::new(node, "Completed"), scope, indent);
    }

    /// A nested run in its own scope: a local read inside an `if` is gone
    /// after its `end`.
    fn block(&mut self, from: &End, scope: &Scope, indent: usize) {
        let mut inner = scope.clone();
        self.run(from, &mut inner, indent + 1);
    }

    /// Reads into locals every value `node` needs that is used more than
    /// once or does work, deepest first, unless this block already holds it.
    fn hoist(&mut self, node: NodeId, scope: &mut Scope, indent: usize) {
        for pin in self.kind(node).inputs {
            if let Some(wire) = self.graph.wire_into(&End::new(node, pin.name)) {
                let from = wire.from.clone();
                self.hoist_value(&from, scope, indent);
            }
        }
    }

    fn hoist_value(&mut self, from: &End, scope: &mut Scope, indent: usize) {
        if scope.contains_key(from) || self.resolving.contains(&from.node) {
            return;
        }
        let kind = self.kind(from.node);
        let Code::Expression { call, .. } = kind.code else {
            return;
        };
        self.resolving.push(from.node);
        self.hoist(from.node, scope, indent);
        self.resolving.pop();
        let reads = self.graph.wires_from(from).count();
        if !call && reads < 2 {
            return;
        }
        let Some((value, _)) = self.value(from, scope) else {
            return;
        };
        let name = self.fresh(&self.local_name(from));
        self.emit(indent, format!("local {name} = {value}"));
        scope.insert(from.clone(), name);
    }

    /// `humanoid` for a lookup of `"Humanoid"`, else the output's own name.
    fn local_name(&self, from: &End) -> String {
        let kind = self.kind(from.node);
        kind.names_local
            .filter(|pin| self.graph.wire_into(&End::new(from.node, pin)).is_none())
            .and_then(|pin| self.graph.value(&End::new(from.node, pin)))
            .filter(|name| is_identifier(name))
            .unwrap_or_else(|| from.pin.clone())
    }

    fn fresh(&mut self, base: &str) -> String {
        let mut stem: String = base
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if stem.is_empty() || stem.starts_with(|c: char| c.is_ascii_digit()) {
            stem.insert(0, 'v');
        }
        let stem = lower_first(&stem);
        let mut name = stem.clone();
        let mut n = 2;
        while TAKEN.contains(&name.as_str()) || self.names.contains(&name) {
            name = format!("{stem}{n}");
            n += 1;
        }
        self.names.insert(name.clone());
        name
    }

    /// The value an output gives, as an expression.
    fn value(&mut self, from: &End, scope: &mut Scope) -> Option<(String, Prec)> {
        if let Some(name) = scope.get(from) {
            return Some((name.clone(), Prec::Atom));
        }
        let kind = self.kind(from.node);
        let Code::Expression { template, prec, .. } = kind.code else {
            self.problem(
                from.node,
                format!(
                    "{} from {} is only available inside its run",
                    from.pin, kind.title
                ),
            );
            return None;
        };
        if self.resolving.contains(&from.node) {
            self.problem(from.node, "This value depends on itself".into());
            return None;
        }
        self.resolving.push(from.node);
        let text = self.fill(from.node, template, prec, scope);
        self.resolving.pop();
        Some((text?, prec))
    }

    /// An input as an expression: what is wired in, or its literal.
    fn input(&mut self, node: NodeId, pin: &str, scope: &mut Scope) -> Option<(String, Prec)> {
        let end = End::new(node, pin);
        if let Some(wire) = self.graph.wire_into(&end) {
            let from = wire.from.clone();
            return self.value(&from, scope);
        }
        let kind = self.kind(node);
        let ty = kind.input(pin).map_or(PinType::Any, |pin| pin.ty);
        let Some(text) = self.graph.value(&end) else {
            self.problem(node, format!("{} needs its {pin} wired", kind.title));
            return None;
        };
        match literal(&text, ty) {
            Ok(value) => Some(value),
            Err(message) => {
                self.problem(node, format!("{}: {message}", kind.title));
                None
            }
        }
    }

    /// Fills a template's `{Pin}` and `{.Pin}` holes. `parent` is the
    /// template's own precedence, against which an operand is wrapped.
    fn fill(
        &mut self,
        node: NodeId,
        template: &str,
        parent: Prec,
        scope: &mut Scope,
    ) -> Option<String> {
        let mut out = String::new();
        let mut rest = template;
        let mut ok = true;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let close = open + rest[open..].find('}').expect("templates close every hole");
            let hole = &rest[open + 1..close];
            rest = &rest[close + 1..];
            if let Some(pin) = hole.strip_prefix('.') {
                let end = End::new(node, pin);
                let literal_name = self
                    .graph
                    .wire_into(&end)
                    .is_none()
                    .then(|| self.graph.value(&end))
                    .flatten()
                    .filter(|name| is_identifier(name));
                match literal_name {
                    Some(name) => out.push_str(&format!(".{name}")),
                    None => match self.input(node, pin, scope) {
                        Some((key, _)) => out.push_str(&format!("[{key}]")),
                        None => ok = false,
                    },
                }
                continue;
            }
            let Some((text, prec)) = self.input(node, hole, scope) else {
                ok = false;
                continue;
            };
            let indexed = rest.starts_with(['.', ':', '[', '(']) || rest.starts_with("{.");
            let needs = match indexed {
                true => prec != Prec::Atom || !indexable(&text),
                false => parent != Prec::Atom && prec != Prec::Atom && prec <= parent,
            };
            out.push_str(&wrap(text, needs));
        }
        out.push_str(rest);
        ok.then_some(out)
    }
}

fn wrap(text: String, needs: bool) -> String {
    match needs {
        true => format!("({text})"),
        false => text,
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub(crate) fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !TAKEN[..21].contains(&text)
}

/// Whether an expression can be indexed or called as written: a name or a
/// call can, a literal string or number cannot.
fn indexable(text: &str) -> bool {
    !text.starts_with(['"', '-']) && !text.starts_with(|c: char| c.is_ascii_digit())
}

/// A typed literal as Luau, or why it is not one.
pub(crate) fn literal(text: &str, ty: PinType) -> Result<(String, Prec), String> {
    let trimmed = text.trim();
    let number = |text: &str| text.parse::<f64>().ok().filter(|n| n.is_finite());
    let prec_of = |text: &str| match text.starts_with('-') {
        true => Prec::Unary,
        false => Prec::Atom,
    };
    match ty {
        PinType::String => Ok((quote(text), Prec::Atom)),
        PinType::Number => match number(trimmed) {
            Some(_) => Ok((trimmed.to_owned(), prec_of(trimmed))),
            None => Err(format!("\"{trimmed}\" is not a number")),
        },
        PinType::Bool => match trimmed {
            "true" | "false" => Ok((trimmed.to_owned(), Prec::Atom)),
            _ => Err(format!("\"{trimmed}\" is not true or false")),
        },
        PinType::Instance => match is_path(trimmed) {
            true => Ok((trimmed.to_owned(), Prec::Atom)),
            false => Err(format!(
                "\"{trimmed}\" is not a path from script, workspace or game"
            )),
        },
        PinType::Any => {
            if number(trimmed).is_some() {
                return Ok((trimmed.to_owned(), prec_of(trimmed)));
            }
            if matches!(trimmed, "true" | "false" | "nil") || is_path(trimmed) {
                return Ok((trimmed.to_owned(), Prec::Atom));
            }
            let inner = trimmed
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .unwrap_or(text);
            Ok((quote(inner), Prec::Atom))
        }
        PinType::Exec | PinType::List => Err("needs a wire".into()),
    }
}

/// `script.Parent`, `workspace.Lava`, `game.Players`: dotted names from one
/// of the three roots a script can always reach.
fn is_path(text: &str) -> bool {
    let mut parts = text.split('.');
    parts
        .next()
        .is_some_and(|root| matches!(root, "script" | "workspace" | "game"))
        && parts.all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
#[path = "codegen/tests.rs"]
mod tests;
