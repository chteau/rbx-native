//! Graph to Luau. Each event becomes a connection (or, for On Start, the
//! top level itself) whose body follows the run-order wires; values are
//! written inline where they are used, except a value read more than once,
//! which is read into a local just before the first statement that needs it.
//!
//! The script's own text is the one truth, so a statement the graph has not
//! changed is written back exactly as it was ([`Origins`]); only the
//! statements an edit touched are generated.
//!
//! A graph that cannot compile — an input left unwired that has no
//! literal, a run that loops into itself, a value used outside the run
//! that gives it — gives back every problem instead, each pinned to its
//! node, and nothing is written.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::catalog::{self, Code, Kind, Pin, PinType, Prec, Syn};
use super::import::{self, raw_lines};
use super::{End, Graph, NodeId};

mod idle;
mod literal;
mod print;
mod syntax;

pub(crate) use idle::idle;
pub(crate) use literal::{indexable, is_identifier, literal, quote};
use literal::{lower_first, TAKEN};
#[allow(unused_imports)] // the importer matches nodes by anchors
pub(crate) use print::{anchors, fingerprint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) node: NodeId,
    pub(crate) message: String,
}

/// Where each statement of an imported script came from, so one the graph
/// has not changed can be written back as it was.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Origins {
    pub(crate) stmts: HashMap<NodeId, Origin>,
    /// Events and start nodes in the order the script held them.
    pub(crate) order: Vec<NodeId>,
    /// What follows the last statement: the final newline, trailing blanks.
    pub(crate) tail: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Origin {
    /// Whitespace between the previous statement's end (or the block or
    /// file start) and this statement's first token.
    pub(crate) lead: String,
    /// The statement from its first token to its last: nested blocks and a
    /// folded `local` that would be read in right before it included.
    pub(crate) text: String,
    /// [`fingerprint`] when the statement was imported.
    pub(crate) print: u64,
    /// Values whose local this text declares, and the names it gave them.
    pub(crate) declares: Vec<(End, String)>,
    /// Values from outside the text that it reads by name.
    pub(crate) needs: Vec<(End, String)>,
}

pub(crate) fn compile(graph: &Graph) -> Result<String, Vec<Problem>> {
    compile_with(graph, &Origins::default())
}

pub(crate) fn compile_with(graph: &Graph, origins: &Origins) -> Result<String, Vec<Problem>> {
    let mut compiler = Compiler::new(graph, origins);
    compiler.reserve_names();
    for node in &graph.nodes {
        if catalog::kind(&node.kind).is_none() {
            compiler.problem(node.id, format!("Unknown node \"{}\"", node.kind));
        }
    }
    for (index, id) in items(graph, origins).into_iter().enumerate() {
        compiler.item(id, index == 0);
    }
    match compiler.problems.is_empty() {
        true => Ok(compiler.finish()),
        false => Err(compiler.problems),
    }
}

/// The events and start nodes in the order they are written: the order the
/// script held them, then the rest by position.
pub(crate) fn items(graph: &Graph, origins: &Origins) -> Vec<NodeId> {
    let is_item = |id: NodeId| graph.kind_of(id).is_some_and(Kind::is_event);
    let mut items: Vec<NodeId> = Vec::new();
    for &id in &origins.order {
        if is_item(id) && !items.contains(&id) {
            items.push(id);
        }
    }
    let mut rest: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| is_item(node.id) && !items.contains(&node.id))
        .collect();
    rest.sort_by(|a, b| (a.y, a.x, a.id).partial_cmp(&(b.y, b.x, b.id)).unwrap());
    items.extend(rest.into_iter().map(|node| node.id));
    items
}

/// The pin a statement's run carries on from, once its own part is done:
/// `None` for what ends a run or is no statement.
pub(super) fn continuation(kind: &Kind) -> Option<&'static str> {
    match kind.code {
        Code::Event(_) | Code::Expression { .. } => None,
        Code::Branch | Code::ForEach | Code::Repeat => Some("Completed"),
        Code::Syntax(Syn::Return) => None,
        Code::Syntax(syn) if syn.is_value() => None,
        Code::Statement(_) | Code::Raw | Code::Syntax(_) => Some(""),
    }
}

/// A node that gives a value rather than doing something.
pub(super) fn is_value(kind: &Kind) -> bool {
    match kind.code {
        Code::Expression { .. } => true,
        Code::Syntax(syn) => syn.is_value(),
        _ => false,
    }
}

/// Which variable holds each value already read in a block.
type Scope = BTreeMap<End, String>;

struct Compiler<'g> {
    graph: &'g Graph,
    origins: &'g Origins,
    out: String,
    /// How deep the statement being written sits.
    indent: usize,
    /// What the next written line starts with, until it has been written.
    lead: Option<String>,
    /// Whether the next item wants a blank line before it.
    gap: bool,
    problems: Vec<Problem>,
    /// Every name given out, so no two locals or parameters share one.
    names: BTreeSet<String>,
    /// The statements of the run being written, to catch one looping back.
    running: Vec<NodeId>,
    /// The values being written, to catch a value loop a hand-edited graph
    /// could hold.
    resolving: Vec<NodeId>,
    /// Writes one node's own code for [`anchors`]: no runs, no locals.
    elide: bool,
}

impl<'g> Compiler<'g> {
    fn new(graph: &'g Graph, origins: &'g Origins) -> Self {
        Compiler {
            graph,
            origins,
            out: String::new(),
            indent: 0,
            lead: None,
            gap: false,
            problems: Vec::new(),
            names: BTreeSet::new(),
            running: Vec::new(),
            resolving: Vec::new(),
            elide: false,
        }
    }

    /// Takes every name the script already uses, so a generated local does
    /// not shadow one: raw code, typed code text and the kept statements.
    fn reserve_names(&mut self) {
        let mut take = |code: &str| self.names.extend(import::names(code));
        for node in &self.graph.nodes {
            if node.kind == "luau" {
                take(&self.graph.value(&End::new(node.id, "Code")).unwrap_or_default());
            }
            for pin in self.graph.pins(node.id).inputs {
                if pin.ty == PinType::Word {
                    take(&self.graph.value(&End::new(node.id, pin.name)).unwrap_or_default());
                }
            }
        }
        for origin in self.origins.stmts.values() {
            take(&origin.text);
        }
    }

    fn finish(mut self) -> String {
        if self.origins.stmts.is_empty() && self.origins.order.is_empty() {
            if !self.out.is_empty() {
                self.out.push('\n');
            }
        } else {
            self.out.push_str(&self.origins.tail);
        }
        self.out
    }

    fn problem(&mut self, node: NodeId, message: String) {
        self.problems.push(Problem { node, message });
    }

    fn kind(&self, node: NodeId) -> &'static Kind {
        self.graph
            .kind_of(node)
            .expect("only nodes of known kinds are compiled")
    }

    /// A fresh line at the current depth; nothing before the very first.
    fn newline(&self) -> String {
        match self.out.is_empty() && self.indent == 0 {
            true => String::new(),
            false => format!("\n{}", "\t".repeat(self.indent)),
        }
    }

    /// What a node's line starts with: its origin's whitespace, else a
    /// fresh line (a blank one first between items).
    fn lead_for(&mut self, node: NodeId) -> String {
        let gap = std::mem::take(&mut self.gap);
        if let Some(origin) = self.origins.stmts.get(&node) {
            return origin.lead.clone();
        }
        let inline = self
            .graph
            .node(node)
            .is_some_and(|n| n.values.get("@inline").is_some_and(|v| v == "1"));
        if inline {
            return " ".into();
        }
        let mut lead = self.newline();
        if gap && self.indent == 0 && !lead.is_empty() {
            lead.insert(0, '\n');
        }
        lead
    }

    /// Writes a line, preceded by the pending lead or a fresh line.
    fn put(&mut self, text: impl AsRef<str>) {
        let lead = self.lead.take().unwrap_or_else(|| self.newline());
        self.out.push_str(&lead);
        self.out.push_str(text.as_ref());
    }

    /// Whether the origin's text stands as it is, and if so writes it.
    fn splice(&mut self, node: NodeId, scope: &mut Scope) -> bool {
        let origins = self.origins;
        let Some(origin) = origins.stmts.get(&node) else {
            return false;
        };
        let clean = fingerprint(self.graph, node) == origin.print
            && origin
                .needs
                .iter()
                .all(|(end, name)| scope.get(end) == Some(name));
        if !clean {
            return false;
        }
        self.gap = false;
        self.out.push_str(&origin.lead);
        self.out.push_str(&origin.text);
        for (end, name) in &origin.declares {
            scope.insert(end.clone(), name.clone());
            self.names.insert(name.clone());
        }
        true
    }

    /// One top-level item: an event, or the start node's own run.
    fn item(&mut self, node: NodeId, first: bool) {
        self.gap = !first;
        let mut scope = Scope::new();
        if self.splice(node, &mut scope) {
            return;
        }
        self.event(node, &mut scope);
    }

    fn event(&mut self, node: NodeId, scope: &mut Scope) {
        let Code::Event(signal) = self.kind(node).code else {
            return;
        };
        let Some(signal) = signal else {
            self.run(&End::new(node, ""), scope);
            return;
        };
        let Some(signal) = self.fill(node, signal, Prec::Atom, &mut Scope::new()) else {
            return;
        };
        // Parameters are written up to the last one used or named, so an
        // event nothing reads from stays `function()`.
        let pins = self.graph.pins(node);
        let params: Vec<Pin> = pins
            .outputs
            .into_iter()
            .filter(|pin| pin.ty != PinType::Exec)
            .collect();
        let used = |pin: &Pin| {
            let named = self
                .graph
                .node(node)
                .is_some_and(|n| n.values.contains_key(&format!("@name:{}", pin.name)));
            named
                || self
                    .graph
                    .wires_from(&End::new(node, pin.name))
                    .next()
                    .is_some()
        };
        let upto = params.iter().rposition(used).map_or(0, |i| i + 1);
        let names: Vec<String> = params[..upto]
            .iter()
            .map(|pin| {
                let end = End::new(node, pin.name);
                let name = self.declared(&end);
                scope.insert(end, name.clone());
                name
            })
            .collect();
        self.lead = Some(self.lead_for(node));
        self.put(format!("{signal}:Connect(function({})", names.join(", ")));
        self.indent += 1;
        self.run(&End::new(node, ""), scope);
        self.indent -= 1;
        self.put("end)");
    }

    /// Writes the run leaving `from`, one statement after another.
    fn run(&mut self, from: &End, scope: &mut Scope) {
        if self.elide {
            return;
        }
        let mark = self.running.len();
        let mut at = from.clone();
        loop {
            let next = self.graph.wires_from(&at).next().map(|wire| wire.to.node);
            let Some(next) = next else { break };
            if self.running.contains(&next) {
                self.problem(next, "This run loops back into itself".into());
                break;
            }
            self.running.push(next);
            match self.statement(next, scope) {
                Some(end) => at = end,
                None => break,
            }
        }
        self.running.truncate(mark);
    }

    /// Writes one statement, and says where its run carries on.
    fn statement(&mut self, node: NodeId, scope: &mut Scope) -> Option<End> {
        let kind = self.graph.kind_of(node)?;
        let next = continuation(kind).map(|pin| End::new(node, pin));
        if self.splice(node, scope) {
            return next;
        }
        self.lead = Some(self.lead_for(node));
        self.hoist(node, scope);
        let exec = |pin: &str| End::new(node, pin);
        match kind.code {
            Code::Statement(template) => {
                if let Some(line) = self.fill(node, template, Prec::Atom, scope) {
                    self.put(line);
                }
            }
            Code::Branch => {
                let Some((condition, prec)) = self.input(node, "Condition", scope) else {
                    return next;
                };
                let wired = |pin: &str| self.graph.wires_from(&exec(pin)).next().is_some();
                let (yes, no) = (wired("True"), wired("False"));
                if !yes && no {
                    let condition = wrap(condition, prec < Prec::Unary);
                    self.put(format!("if not {condition} then"));
                    self.block(&exec("False"), scope);
                } else {
                    self.put(format!("if {condition} then"));
                    self.block(&exec("True"), scope);
                    if no {
                        self.put("else");
                        self.block(&exec("False"), scope);
                    }
                }
                self.put("end");
            }
            Code::Raw => {
                let code = self.graph.value(&exec("Code")).unwrap_or_default();
                for (line, verbatim) in raw_lines(&code) {
                    match verbatim || line.trim().is_empty() {
                        true => {
                            let lead = self.lead.take().unwrap_or_else(|| "\n".into());
                            self.out.push_str(&lead);
                            self.out.push_str(line);
                        }
                        false => self.put(line),
                    }
                }
            }
            Code::ForEach => {
                let Some((list, _)) = self.input(node, "List", scope) else {
                    return next;
                };
                let mut inner = scope.clone();
                let index = self.bind(node, "Index", &mut inner);
                let item = self.bind(node, "Item", &mut inner);
                self.put(format!("for {index}, {item} in {list} do"));
                self.body(&exec("Loop"), inner);
            }
            Code::Repeat => {
                let Some((count, _)) = self.input(node, "Count", scope) else {
                    return next;
                };
                let mut inner = scope.clone();
                let index = self.bind(node, "Index", &mut inner);
                self.put(format!("for {index} = 1, {count} do"));
                self.body(&exec("Loop"), inner);
            }
            Code::Syntax(syn) => self.syntax(node, syn, scope),
            Code::Event(_) | Code::Expression { .. } => return None,
        }
        next
    }

    /// A loop's own output, named for the loop's body alone.
    fn bind(&mut self, node: NodeId, pin: &str, inner: &mut Scope) -> String {
        let end = End::new(node, pin);
        let name = self.declared(&end);
        inner.insert(end, name.clone());
        name
    }

    /// A loop's body in the scope it was given, then its `end`.
    fn body(&mut self, from: &End, mut inner: Scope) {
        self.indent += 1;
        self.run(from, &mut inner);
        self.indent -= 1;
        self.put("end");
    }

    /// A nested run in its own scope: a local read inside an `if` is gone
    /// after its `end`.
    fn block(&mut self, from: &End, scope: &Scope) {
        let mut inner = scope.clone();
        self.indent += 1;
        self.run(from, &mut inner);
        self.indent -= 1;
    }

    /// Whether an input's value may be read into a local first: not the
    /// place an assignment writes to, and not a condition that must see the
    /// loop body's locals.
    fn hoists(&self, node: NodeId, pin: &str) -> bool {
        match self.kind(node).code {
            Code::Syntax(Syn::Assign | Syn::Compound) => !pin.starts_with("Target"),
            Code::Syntax(Syn::RepeatUntil | Syn::While) => pin != "Condition",
            _ => true,
        }
    }

    /// Reads into locals every value `node` needs that is read more than
    /// once, deepest first, unless this block already holds it.
    fn hoist(&mut self, node: NodeId, scope: &mut Scope) {
        if self.elide {
            return;
        }
        for pin in self.graph.pins(node).inputs {
            if pin.ty != PinType::Exec && self.hoists(node, pin.name) {
                self.hoist_pin(node, pin.name, scope);
            }
        }
    }

    fn hoist_pin(&mut self, node: NodeId, pin: &str, scope: &mut Scope) {
        if let Some(wire) = self.graph.wire_into(&End::new(node, pin)) {
            let from = wire.from.clone();
            self.hoist_value(&from, scope);
        }
    }

    fn hoist_value(&mut self, from: &End, scope: &mut Scope) {
        if scope.contains_key(from) || self.resolving.contains(&from.node) {
            return;
        }
        let Some(kind) = self.graph.kind_of(from.node).filter(|kind| is_value(kind)) else {
            return;
        };
        self.resolving.push(from.node);
        self.hoist(from.node, scope);
        self.resolving.pop();
        // ponytail: a bare name or literal is as cheap to read twice.
        let plain = matches!(kind.code, Code::Syntax(Syn::Get | Syn::Literal));
        if plain || self.graph.wires_from(from).count() < 2 {
            return;
        }
        let Some((value, _)) = self.value(from, scope) else {
            return;
        };
        let name = self.declared(from);
        self.put(format!("local {name} = {value}"));
        scope.insert(from.clone(), name);
    }

    /// The name an output's value is kept under: the one it was imported
    /// with, else `humanoid` for a lookup of `"Humanoid"`, else its own name.
    fn declared(&mut self, from: &End) -> String {
        let kept = self
            .graph
            .node(from.node)
            .and_then(|node| node.values.get(&format!("@name:{}", from.pin)))
            .filter(|name| !name.is_empty())
            .cloned();
        if let Some(name) = kept {
            self.names.insert(name.clone());
            return name;
        }
        // An anchor's text must not depend on how many came before it.
        if self.elide {
            return from.pin.clone();
        }
        let kind = self.kind(from.node);
        let base = kind
            .names_local
            .filter(|pin| self.graph.wire_into(&End::new(from.node, pin)).is_none())
            .and_then(|pin| self.graph.value(&End::new(from.node, pin)))
            .filter(|name| is_identifier(name))
            .unwrap_or_else(|| from.pin.clone());
        self.fresh(&base)
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
        let kind = self.graph.kind_of(from.node)?;
        if !is_value(kind) {
            if self.elide {
                let named = self
                    .graph
                    .node(from.node)
                    .and_then(|node| node.values.get(&format!("@name:{}", from.pin)));
                return Some((named.unwrap_or(&from.pin).clone(), Prec::Atom));
            }
            self.problem(
                from.node,
                format!(
                    "{} from {} is only available inside its run",
                    from.pin, kind.title
                ),
            );
            return None;
        }
        if self.resolving.contains(&from.node) {
            self.problem(from.node, "This value depends on itself".into());
            return None;
        }
        self.resolving.push(from.node);
        let result = match kind.code {
            Code::Expression { template, prec, .. } => self
                .fill(from.node, template, prec, scope)
                .map(|text| (text, prec)),
            Code::Syntax(syn) => self.syntax_value(from.node, syn, scope),
            _ => None,
        };
        self.resolving.pop();
        // Comments written inside the expression, before and after it.
        let node = self.graph.node(from.node)?;
        let around = |key: &str| node.values.get(key).cloned().unwrap_or_default();
        result.map(|(text, prec)| {
            (format!("{}{text}{}", around("@lead"), around("@trail")), prec)
        })
    }

    /// An input as an expression: what is wired in, or its literal.
    fn input(&mut self, node: NodeId, pin: &str, scope: &mut Scope) -> Option<(String, Prec)> {
        let end = End::new(node, pin);
        if let Some(wire) = self.graph.wire_into(&end) {
            let from = wire.from.clone();
            return self.value(&from, scope);
        }
        let kind = self.kind(node);
        let ty = self
            .graph
            .input_pin(node, pin)
            .map_or(PinType::Any, |pin| pin.ty);
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
            // Nothing before the hole: it is the left operand.
            let left = out.trim().is_empty();
            let needs = match indexed {
                true => prec != Prec::Atom || !indexable(&text),
                false => parent != Prec::Atom && operand_needs(prec, parent, left),
            };
            // `- -x` must not become a comment.
            let needs = needs || (out.ends_with('-') && text.starts_with('-'));
            out.push_str(&wrap(text, needs));
        }
        out.push_str(rest);
        ok.then_some(out)
    }
}

/// Whether an operand of precedence `prec` needs brackets under an operator
/// of precedence `parent`. Equal precedence groups to the left, except for
/// the operators that group to the right.
fn operand_needs(prec: Prec, parent: Prec, left: bool) -> bool {
    match prec {
        Prec::Atom => false,
        _ if prec < parent => true,
        _ if prec == parent => left == parent.right_assoc(),
        _ => false,
    }
}

fn wrap(text: String, needs: bool) -> String {
    match needs {
        true => format!("({text})"),
        false => text,
    }
}

#[cfg(test)]
#[path = "codegen/tests.rs"]
mod tests;
#[cfg(test)]
mod generic_tests;
