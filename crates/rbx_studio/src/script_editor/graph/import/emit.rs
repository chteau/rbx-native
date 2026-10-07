//! The tree as a graph. Every construct becomes generic syntax nodes; a
//! catalog node stands in only where it writes the same tokens. Each
//! top-level piece is built, compiled on its own and compared with its
//! source; one that comes out different is built again with less cleverness,
//! down to a Luau Code node that keeps the text as it was.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::script_editor::graph::catalog::{self, PinType};
use crate::script_editor::graph::codegen::{self, Origin, Origins};
use crate::script_editor::graph::{End, Graph, NodeId};

use super::pattern::{self, Hit};
use super::tree::{Block, Cm, Fld, Item, Key, Style, Tree, E, F, K, S, SK};

thread_local! {
    /// The pieces that only a Luau Code node could hold, for the corpus test.
    static FALLBACKS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The pieces kept as Luau Code since the last call, each with what the
/// graph made of it.
#[cfg(test)]
pub(super) fn take_fallbacks() -> Vec<String> {
    FALLBACKS.with(|f| std::mem::take(&mut *f.borrow_mut()))
}

/// Where two texts first part, for the fallback record.
fn first_diff(a: &str, b: &str) -> String {
    for (n, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}: {x:?} vs {y:?}", n + 1);
        }
    }
    format!(
        "length {} vs {} lines",
        a.lines().count(),
        b.lines().count()
    )
}

#[derive(Clone)]
enum Bind {
    /// Read by name.
    Plain,
    /// Read from a value node's output.
    Wired(End),
}

/// What one statement draws on and declares, for its origin.
#[derive(Default)]
struct Coll {
    needs: Vec<(End, String)>,
    owns: HashSet<End>,
}

enum V {
    Text(String),
    End(End),
}

struct Mark {
    nodes: usize,
    wires: usize,
    used: usize,
}

struct B<'a> {
    src: &'a str,
    tree: &'a Tree,
    g: Graph,
    o: Origins,
    lead: HashMap<usize, Vec<Cm>>,
    trail: HashMap<usize, Vec<Cm>>,
    used: Vec<usize>,
    used_set: HashSet<usize>,
    scopes: Vec<HashMap<String, Bind>>,
    colls: Vec<Coll>,
    names_of: HashMap<End, String>,
    fold: bool,
    recog: bool,
    failed: bool,
    /// The run top-level code goes into, and where it continues.
    start: Option<NodeId>,
    tail: Option<End>,
    out: String,
}

pub(super) fn build(src: &str, tree: &Tree) -> (Graph, Origins) {
    let mut b = B {
        src,
        tree,
        g: Graph::default(),
        o: Origins::default(),
        lead: HashMap::new(),
        trail: HashMap::new(),
        used: Vec::new(),
        used_set: HashSet::new(),
        scopes: vec![HashMap::new()],
        colls: Vec::new(),
        names_of: HashMap::new(),
        fold: true,
        recog: true,
        failed: false,
        start: None,
        tail: None,
        out: String::new(),
    };
    b.comment_maps();
    b.top();
    let ids: Vec<NodeId> = b.o.stmts.keys().copied().collect();
    for id in ids {
        let print = codegen::fingerprint(&b.g, id);
        if let Some(origin) = b.o.stmts.get_mut(&id) {
            origin.print = print;
        }
    }
    b.o.tail = src[tree.block.items.last().map_or(0, |i| i.span().1)..].to_owned();
    (b.g, b.o)
}

fn has_word(text: &str, name: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == name)
}

fn is_block_comment(text: &str) -> bool {
    text.strip_prefix("--[")
        .is_some_and(|rest| rest.trim_start_matches('=').starts_with('['))
}

/// How a stretch of code uses a name.
#[derive(Default)]
struct Use {
    reads: usize,
    /// It is written, redeclared or named where a read cannot be told apart.
    blocked: bool,
}

impl B<'_> {
    fn text(&self, lo: usize, hi: usize) -> String {
        self.src.get(lo..hi).unwrap_or_default().to_owned()
    }

    // Comments inside expressions.

    /// Comments that sit inside a statement, between its tokens, by the
    /// token they come before and the one they follow.
    fn comment_maps(&mut self) {
        let tokens = &self.tree.tokens;
        for c in &self.tree.comments {
            if self.tree.claimed.contains(&c.lo) {
                continue;
            }
            let next = tokens.partition_point(|t| t.0 < c.hi);
            if let Some(t) = tokens.get(next) {
                self.lead.entry(t.0).or_default().push(*c);
            }
            let before = tokens.partition_point(|t| t.1 <= c.lo);
            if let Some(t) = before.checked_sub(1).and_then(|i| tokens.get(i)) {
                self.trail.entry(t.1).or_default().push(*c);
            }
        }
    }

    fn take(&mut self, trailing: bool, at: usize) -> Vec<Cm> {
        let map = if trailing { &self.trail } else { &self.lead };
        let found: Vec<Cm> = map
            .get(&at)
            .into_iter()
            .flatten()
            .filter(|c| !self.used_set.contains(&c.lo))
            .copied()
            .collect();
        for c in &found {
            self.used_set.insert(c.lo);
            self.used.push(c.lo);
        }
        found
    }

    fn lead_text(&self, comments: &[Cm]) -> String {
        comments
            .iter()
            .map(|c| {
                let text = &self.src[c.lo..c.hi];
                format!("{text}{}", if is_block_comment(text) { " " } else { "\n" })
            })
            .collect()
    }

    fn trail_text(&self, comments: &[Cm]) -> String {
        comments
            .iter()
            .map(|c| {
                let text = &self.src[c.lo..c.hi];
                format!(" {text}{}", if is_block_comment(text) { "" } else { "\n" })
            })
            .collect()
    }

    // Graph building.

    fn mark(&self) -> Mark {
        Mark {
            nodes: self.g.nodes.len(),
            wires: self.g.wires.len(),
            used: self.used.len(),
        }
    }

    fn rollback(&mut self, m: &Mark) {
        if let Some(first) = self.g.nodes.get(m.nodes).map(|n| n.id) {
            self.o.stmts.retain(|id, _| *id < first);
        }
        self.g.nodes.truncate(m.nodes);
        self.g.wires.truncate(m.wires);
        for lo in self.used.drain(m.used..) {
            self.used_set.remove(&lo);
        }
    }

    fn add(&mut self, key: &str) -> NodeId {
        match catalog::kind(key) {
            Some(kind) => self.g.add(kind, [0.0, 0.0]),
            None => {
                self.failed = true;
                0
            }
        }
    }

    fn set(&mut self, node: NodeId, key: &str, value: impl Into<String>) {
        if let Some(n) = self.g.node_mut(node) {
            n.values.insert(key.to_owned(), value.into());
        }
    }

    fn word(&mut self, node: NodeId, pin: &str, value: &str) {
        self.g.set_value(&End::new(node, pin), value.to_owned());
    }

    fn link(&mut self, from: End, to: End) {
        if self.g.connect(from, to).is_err() {
            self.failed = true;
        }
    }

    // Names.

    fn lookup(&self, name: &str) -> Bind {
        self.scopes
            .iter()
            .rev()
            .find_map(|frame| frame.get(name))
            .cloned()
            .unwrap_or(Bind::Plain)
    }

    fn declare(&mut self, name: &str, bind: Bind) {
        if let Bind::Wired(end) = &bind {
            for c in &mut self.colls {
                c.owns.insert(end.clone());
            }
            self.names_of.insert(end.clone(), name.to_owned());
        }
        if let Some(frame) = self.scopes.last_mut() {
            frame.insert(name.to_owned(), bind);
        }
    }

    fn declare_all(&mut self, names: &[String]) {
        for name in names {
            self.declare(name, Bind::Plain);
        }
    }

    fn read(&mut self, end: &End, name: &str) {
        for c in &mut self.colls {
            if !c.owns.contains(end) && !c.needs.iter().any(|(e, _)| e == end) {
                c.needs.push((end.clone(), name.to_owned()));
            }
        }
    }

    // Reads and writes of a name, for folding.

    fn scan_expr(&self, e: &E, name: &str, funcs: bool, u: &mut Use) {
        let sub = |x: &E, u: &mut Use| self.scan_expr(x, name, funcs, u);
        match &e.k {
            K::Name(n) => u.reads += usize::from(n == name),
            K::Lit => {}
            K::Field(a, _) | K::Un(_, a) | K::Paren(a) => sub(a, u),
            K::Cast(a, ty) => {
                u.blocked |= has_word(ty, name);
                sub(a, u);
            }
            K::Index(a, b) | K::Bin(a, _, b) => {
                sub(a, u);
                sub(b, u);
            }
            K::Call(f, args, _) => {
                sub(f, u);
                args.iter().for_each(|a| sub(a, u));
            }
            K::Method(o, _, args, _) => {
                sub(o, u);
                args.iter().for_each(|a| sub(a, u));
            }
            K::Func(f) => {
                if funcs {
                    self.scan_func(f, name, u);
                }
            }
            K::Table(fields) => {
                for f in fields {
                    if let Key::Expr(k) = &f.key {
                        sub(k, u);
                    }
                    sub(&f.value, u);
                }
            }
            K::IfExp(arms, els) => {
                for (c, t) in arms {
                    sub(c, u);
                    sub(t, u);
                }
                sub(els, u);
            }
            K::Interp(_, es) => es.iter().for_each(|x| sub(x, u)),
            K::Bad => u.blocked |= has_word(&self.src[e.lo..e.hi], name),
        }
    }

    fn scan_func(&self, f: &F, name: &str, u: &mut Use) {
        u.blocked |=
            has_word(&f.params, name) || has_word(&f.generics, name) || has_word(&f.returns, name);
        self.scan_block(&f.body, 0, f.body.items.len(), name, u);
    }

    fn scan_block(&self, b: &Block, from: usize, to: usize, name: &str, u: &mut Use) {
        for item in &b.items[from..to] {
            if let Item::S(s) = item {
                self.scan_stmt(s, name, u);
            }
        }
    }

    fn scan_target(&self, t: &E, name: &str, u: &mut Use) {
        match &t.k {
            K::Name(n) if n == name => u.blocked = true,
            _ => self.scan_expr(t, name, true, u),
        }
    }

    fn scan_stmt(&self, s: &S, name: &str, u: &mut Use) {
        let ex = |e: &E, u: &mut Use| self.scan_expr(e, name, true, u);
        match &s.k {
            SK::Local {
                names,
                text,
                typed,
                vals,
            } => {
                u.blocked |= names.iter().any(|n| n == name) || (*typed && has_word(text, name));
                vals.iter().for_each(|e| ex(e, u));
            }
            SK::Assign(ts, vs) => {
                ts.iter().for_each(|t| self.scan_target(t, name, u));
                vs.iter().for_each(|e| ex(e, u));
            }
            SK::Compound(t, _, v) => {
                self.scan_target(t, name, u);
                ex(v, u);
            }
            SK::Call(e) => ex(e, u),
            SK::If(arms, els) => {
                for (c, b) in arms {
                    ex(c, u);
                    self.scan_block(b, 0, b.items.len(), name, u);
                }
                if let Some(b) = els {
                    self.scan_block(b, 0, b.items.len(), name, u);
                }
            }
            SK::While(c, b) | SK::Repeat(b, c) => {
                ex(c, u);
                self.scan_block(b, 0, b.items.len(), name, u);
            }
            SK::ForCount {
                var,
                name: n,
                from,
                to,
                step,
                body,
            } => {
                u.blocked |= n == name || has_word(var, name);
                ex(from, u);
                ex(to, u);
                step.iter().for_each(|e| ex(e, u));
                self.scan_block(body, 0, body.items.len(), name, u);
            }
            SK::ForEach {
                vars,
                names,
                vals,
                body,
            } => {
                u.blocked |= names.iter().any(|n| n == name) || has_word(vars, name);
                vals.iter().for_each(|e| ex(e, u));
                self.scan_block(body, 0, body.items.len(), name, u);
            }
            SK::Do(b) => self.scan_block(b, 0, b.items.len(), name, u),
            SK::Function { root, f, .. } => {
                u.blocked |= root == name;
                self.scan_func(f, name, u);
            }
            SK::Return(es) => es.iter().for_each(|e| ex(e, u)),
            SK::Break | SK::Continue => {}
            SK::Type | SK::Bad => u.blocked |= has_word(&self.src[s.lo..s.hi], name),
        }
    }

    /// Whether a statement reads `name` in the inputs of its own node, not
    /// in a block or a function it holds.
    fn reads_own(&self, s: &S, name: &str) -> bool {
        let mut u = Use::default();
        let ex = |e: &E, u: &mut Use| self.scan_expr(e, name, false, u);
        match &s.k {
            SK::Local { vals, .. } | SK::Return(vals) | SK::ForEach { vals, .. } => {
                vals.iter().for_each(|e| ex(e, &mut u));
            }
            SK::Assign(_, vs) => vs.iter().for_each(|e| ex(e, &mut u)),
            SK::Compound(_, _, v) => ex(v, &mut u),
            SK::Call(e) => ex(e, &mut u),
            SK::If(arms, _) => arms.iter().for_each(|(c, _)| ex(c, &mut u)),
            SK::ForCount { from, to, step, .. } => {
                ex(from, &mut u);
                ex(to, &mut u);
                step.iter().for_each(|e| ex(e, &mut u));
            }
            _ => {}
        }
        u.reads > 0
    }

    /// Whether `items[k]` is a local to draw as one shared value: read at
    /// least twice by what follows (the very next statement among the
    /// readers), and never written.
    fn is_fold(&self, items: &[Item], k: usize, limit: usize) -> bool {
        let Some(Item::S(S {
            k:
                SK::Local {
                    names,
                    typed: false,
                    vals,
                    ..
                },
            ..
        })) = items.get(k)
        else {
            return false;
        };
        let ([name], [value]) = (names.as_slice(), vals.as_slice()) else {
            return false;
        };
        if matches!(value.k, K::Name(_) | K::Lit | K::Bad) || k + 1 >= limit {
            return false;
        }
        let Item::S(next) = &items[k + 1] else {
            return false;
        };
        if !self.reads_own(next, name) {
            return false;
        }
        let mut u = Use::default();
        for item in &items[k + 1..limit] {
            if let Item::S(s) = item {
                self.scan_stmt(s, name, &mut u);
            }
        }
        u.reads >= 2 && !u.blocked
    }

    fn last_reader(&self, items: &[Item], k: usize, limit: usize) -> usize {
        let name = match &items[k] {
            Item::S(S {
                k: SK::Local { names, .. },
                ..
            }) => names[0].as_str(),
            _ => return k,
        };
        (k + 1..limit)
            .rev()
            .find(|&j| {
                let mut u = Use::default();
                if let Item::S(s) = &items[j] {
                    self.scan_stmt(s, name, &mut u);
                }
                u.reads > 0
            })
            .unwrap_or(k)
    }

    /// Whether the body writes or redeclares `name`.
    fn writes(&self, b: &Block, name: &str) -> bool {
        let mut u = Use::default();
        self.scan_block(b, 0, b.items.len(), name, &mut u);
        u.blocked
    }

    // Top level.

    fn top(&mut self) {
        let tree = self.tree;
        let items = &tree.block.items;
        let events: Vec<bool> = items
            .iter()
            .map(|item| matches!(item, Item::S(S { k: SK::Call(e), .. }) if pattern::event(self.src, e).is_some()))
            .collect();
        let n = items.len();
        let mut i = 0;
        while i < n {
            let limit = (i + 1..n).find(|&j| events[j]).unwrap_or(n);
            let mut end = if events[i] {
                i + 1
            } else {
                self.extent(&tree.block, i, limit)
            };
            let mut levels = vec![(true, true), (false, true), (false, false)].into_iter();
            loop {
                let Some((fold, recog)) = levels.next() else {
                    if end > i + 1 {
                        end = i + 1;
                        levels = vec![(false, true), (false, false)].into_iter();
                        continue;
                    }
                    self.fallback(&tree.block, i);
                    i += 1;
                    break;
                };
                (self.fold, self.recog) = (fold, recog);
                if self.attempt(&tree.block, i, end, events[i] && recog) {
                    i = end;
                    break;
                }
            }
        }
    }

    /// How far the piece starting at `i` reaches: past every read of a
    /// shared local, and to the end after a `return`.
    fn extent(&self, b: &Block, i: usize, limit: usize) -> usize {
        let items = &b.items;
        let (mut end, mut k) = (i + 1, i);
        while k < end {
            if matches!(
                items[k],
                Item::S(S {
                    k: SK::Return(_),
                    ..
                })
            ) {
                end = limit;
            }
            if self.is_fold(items, k, limit) {
                end = end.max(self.last_reader(items, k, limit) + 1);
            }
            k += 1;
        }
        end.min(limit)
    }

    /// Builds `items[i..end]` as one piece, checks it against its source and
    /// joins it to the run. False, with nothing left behind, when it differs.
    fn attempt(&mut self, b: &Block, i: usize, end: usize, event: bool) -> bool {
        let mark = self.mark();
        self.failed = false;
        self.scopes = vec![HashMap::new()];
        self.colls.clear();
        let (lo, hi) = (b.items[i].span().0, b.items[end - 1].span().1);
        let built = if event {
            self.event(b, i).map(|node| (node, None))
        } else {
            self.seq(b, i, end).map(|(head, tail)| (head, Some(tail)))
        };
        let ok = !self.failed
            && built.is_some()
            && self.verify(&mark, built.as_ref().map(|b| b.0), event, lo, hi);
        if !ok {
            self.rollback(&mark);
            return false;
        }
        let Some((head, tail)) = built else {
            return false;
        };
        match tail {
            Some(tail) => {
                self.join(head);
                self.tail = tail;
            }
            None => {
                self.o.order.push(head);
                self.start = None;
                self.tail = None;
            }
        }
        true
    }

    /// Hangs a run's first node under the script's start node, making one
    /// when the last run ended or there is none yet.
    fn join(&mut self, head: NodeId) {
        let tail = match (self.start, self.tail.take()) {
            (Some(_), Some(tail)) => tail,
            _ => {
                let start = self.add("start");
                self.o.order.push(start);
                self.start = Some(start);
                End::new(start, "")
            }
        };
        self.link(tail, End::new(head, ""));
    }

    fn verify(
        &mut self,
        mark: &Mark,
        head: Option<NodeId>,
        event: bool,
        lo: usize,
        hi: usize,
    ) -> bool {
        let Some(head) = head else {
            return false;
        };
        let mut sub = Graph {
            nodes: self.g.nodes[mark.nodes..].to_vec(),
            wires: self.g.wires[mark.wires..].to_vec(),
            ..Graph::default()
        };
        if !event {
            let Some(kind) = catalog::kind("start") else {
                return false;
            };
            let start = sub.add(kind, [0.0, 0.0]);
            if sub
                .connect(End::new(start, ""), End::new(head, ""))
                .is_err()
            {
                return false;
            }
        }
        match codegen::compile(&sub) {
            Ok(out) => {
                self.out = out;
                super::same(&self.out, &self.src[lo..hi])
            }
            Err(_) => {
                self.out.clear();
                false
            }
        }
    }

    /// The piece as Luau Code, as written: the importer's last resort.
    fn fallback(&mut self, b: &Block, i: usize) {
        let (lo, hi) = b.items[i].span();
        let text = self.src[lo..hi]
            .trim_end_matches([';', ' ', '\t', '\r', '\n'])
            .to_owned();
        FALLBACKS.with(|f| {
            f.borrow_mut().push(format!(
                "{} ... first difference: {}\n--- compiled:\n{}",
                text.lines().next().unwrap_or_default(),
                first_diff(&text, &self.out),
                self.out
            ))
        });
        let node = self.luau(&text);
        self.origin(b, i, i + 1, node, Vec::new(), Vec::new());
        self.join(node);
        self.tail = Some(End::new(node, ""));
    }

    fn luau(&mut self, text: &str) -> NodeId {
        let node = self.add("luau");
        self.word(node, "Code", text);
        node
    }

    fn origin(
        &mut self,
        b: &Block,
        i: usize,
        next: usize,
        node: NodeId,
        declares: Vec<(End, String)>,
        needs: Vec<(End, String)>,
    ) {
        let prev = if i == 0 {
            b.lo
        } else {
            b.items[i - 1].span().1
        };
        let (lo, _) = b.items[i].span();
        let hi = b.items[next - 1].span().1;
        self.o.stmts.insert(
            node,
            Origin {
                lead: self.text(prev, lo),
                text: self.text(lo, hi),
                print: 0,
                declares,
                needs,
            },
        );
    }

    // Events.

    fn event(&mut self, b: &Block, i: usize) -> Option<NodeId> {
        let Item::S(S {
            k: SK::Call(call), ..
        }) = &b.items[i]
        else {
            return None;
        };
        let (hit, f) = pattern::event(self.src, call)?;
        let node = self.catalog_node(&hit);
        let outs: Vec<&'static str> = self
            .g
            .pins(node)
            .outputs
            .iter()
            .filter(|p| p.ty != PinType::Exec)
            .map(|p| p.name)
            .collect();
        self.scopes.push(HashMap::new());
        for (name, pin) in f.names.iter().zip(outs) {
            self.set(node, &format!("@name:{pin}"), name.clone());
            // A parameter the body assigns is read by name.
            let bind = match self.writes(&f.body, name) {
                true => Bind::Plain,
                false => Bind::Wired(End::new(node, pin)),
            };
            self.declare(name, bind);
        }
        if let Some((head, _)) = self.seq_block(&f.body) {
            self.link(End::new(node, ""), End::new(head, ""));
        }
        self.scopes.pop();
        self.origin(b, i, i + 1, node, Vec::new(), Vec::new());
        Some(node)
    }

    // Statements.

    fn seq_block(&mut self, b: &Block) -> Option<(NodeId, Option<End>)> {
        self.seq(b, 0, b.items.len())
    }

    /// The run of `items[from..to]`: its first node, and the pin the run
    /// goes on from (none after a `return`).
    fn seq(&mut self, b: &Block, from: usize, to: usize) -> Option<(NodeId, Option<End>)> {
        let mut head: Option<NodeId> = None;
        let mut tail: Option<End> = None;
        let mut i = from;
        while i < to {
            let (node, next) = self.item(b, i, to);
            match head {
                None => head = Some(node),
                Some(_) => {
                    if let Some(t) = tail.take() {
                        self.link(t, End::new(node, ""));
                    }
                }
            }
            tail = self
                .g
                .kind_of(node)
                .and_then(codegen::continuation)
                .map(|pin| End::new(node, pin));
            i = next;
        }
        head.map(|h| (h, tail))
    }

    /// One statement, comment or shared-local group: its first node, and
    /// the index of the next item.
    fn item(&mut self, b: &Block, i: usize, to: usize) -> (NodeId, usize) {
        if let Item::C(c) = &b.items[i] {
            let node = self.add("comment");
            self.word(node, "Text", &self.text(c.lo, c.hi));
            if c.inline {
                self.set(node, "@inline", "1");
            }
            self.origin(b, i, i + 1, node, Vec::new(), Vec::new());
            return (node, i + 1);
        }
        let mut j = i;
        while self.fold && j < to && self.is_fold(&b.items, j, to) {
            j += 1;
        }
        self.colls.push(Coll::default());
        let mut declares = Vec::new();
        for k in i..j {
            if let Some(d) = self.fold_local(&b.items[k]) {
                declares.push(d);
            }
        }
        let (node, next) = self.stmt(b, j);
        let coll = self.colls.pop().unwrap_or_default();
        self.origin(b, i, next, node, declares, coll.needs);
        (node, next)
    }

    /// `local name = value` drawn as the value alone, shared by its readers.
    fn fold_local(&mut self, item: &Item) -> Option<(End, String)> {
        let Item::S(S {
            k: SK::Local { names, vals, .. },
            ..
        }) = item
        else {
            return None;
        };
        let end = match self.expr(&vals[0], PinType::Any, false) {
            V::End(end) => end,
            V::Text(_) => return None,
        };
        self.set(end.node, &format!("@name:{}", end.pin), names[0].clone());
        self.declare(&names[0], Bind::Wired(end.clone()));
        Some((end, names[0].clone()))
    }

    fn catalog_node(&mut self, hit: &Hit<'_>) -> NodeId
where {
        let node = self.add(hit.kind.key);
        for (pin, e) in &hit.holes {
            self.bind_tree(node, pin, e);
        }
        for (pin, word) in &hit.words {
            self.word(node, pin, word);
        }
        node
    }

    /// `items[j]` as nodes: its first node, the next index, and the end of
    /// its text (comments after a `return` included).
    fn stmt(&mut self, b: &Block, j: usize) -> (NodeId, usize) {
        let Item::S(s) = &b.items[j] else {
            return (self.add("comment"), j + 1);
        };
        if self.recog {
            for hit in pattern::statement(self.src, s) {
                let (was, m) = (self.failed, self.mark());
                self.failed = false;
                let node = self.catalog_node(&hit);
                if !self.failed {
                    self.failed = was;
                    return (node, j + 1);
                }
                self.rollback(&m);
                self.failed = was;
            }
        }
        let mut next = j + 1;
        let node = match &s.k {
            SK::Local {
                names, text, vals, ..
            } => {
                let node = self.add("local");
                self.word(node, "Names", text);
                self.set(node, "#values", vals.len().to_string());
                for (n, e) in vals.iter().enumerate() {
                    self.bind(node, &format!("Value {}", n + 1), e);
                }
                self.declare_all(names);
                node
            }
            SK::Assign(ts, vs) => {
                let node = self.add("assign");
                self.set(node, "#targets", ts.len().to_string());
                self.set(node, "#values", vs.len().to_string());
                for (n, e) in ts.iter().enumerate() {
                    self.bind(node, &format!("Target {}", n + 1), e);
                }
                for (n, e) in vs.iter().enumerate() {
                    self.bind(node, &format!("Value {}", n + 1), e);
                }
                node
            }
            SK::Compound(t, op, v) => {
                let node = self.add("compound");
                self.bind(node, "Target", t);
                self.word(node, "Op", op);
                self.bind(node, "Value", v);
                node
            }
            SK::Call(e) => self.call_stmt(e, s),
            SK::If(arms, els) => {
                let node = self.add("if");
                self.set(node, "#branches", arms.len().to_string());
                for (n, (c, body)) in arms.iter().enumerate() {
                    self.bind(node, &format!("Condition {}", n + 1), c);
                    self.nested(node, &format!("Then {}", n + 1), body, &[]);
                }
                if let Some(body) = els {
                    if body.items.is_empty() {
                        self.set(node, "@else", "1");
                    }
                    self.nested(node, "Else", body, &[]);
                }
                node
            }
            SK::While(c, body) => {
                let node = self.add("while");
                self.bind(node, "Condition", c);
                self.nested(node, "Do", body, &[]);
                node
            }
            SK::Repeat(body, c) => {
                let node = self.add("repeat");
                self.scopes.push(HashMap::new());
                if let Some((head, _)) = self.seq_block(body) {
                    self.link(End::new(node, "Do"), End::new(head, ""));
                }
                self.bind(node, "Condition", c);
                self.scopes.pop();
                node
            }
            SK::ForCount {
                var,
                name,
                from,
                to,
                step,
                body,
            } => {
                let node = self.add("for_count");
                self.word(node, "Variable", var);
                self.bind(node, "From", from);
                self.bind(node, "To", to);
                if let Some(step) = step {
                    self.bind(node, "Step", step);
                }
                self.nested(node, "Do", body, std::slice::from_ref(name));
                node
            }
            SK::ForEach {
                vars,
                names,
                vals,
                body,
            } => {
                let node = self.add("for_each");
                self.word(node, "Variables", vars);
                self.set(node, "#values", vals.len().to_string());
                for (n, e) in vals.iter().enumerate() {
                    self.bind(node, &format!("In {}", n + 1), e);
                }
                self.nested(node, "Do", body, names);
                node
            }
            SK::Do(body) => {
                let node = self.add("do");
                self.nested(node, "Do", body, &[]);
                node
            }
            SK::Function {
                attrs,
                local,
                name,
                f,
                ..
            } => {
                let node = self.add("function");
                if *local {
                    self.set(node, "@local", "1");
                    self.declare(name, Bind::Plain);
                }
                self.word(node, "Name", name);
                self.func(node, f, attrs);
                node
            }
            SK::Return(es) => {
                let node = self.add("return");
                self.set(node, "#values", es.len().to_string());
                for (n, e) in es.iter().enumerate() {
                    self.bind(node, &format!("Value {}", n + 1), e);
                }
                // Comments after it: nothing can follow a return to hold them.
                if j + 1 < b.items.len() {
                    let after = self.text(s.hi, b.items[b.items.len() - 1].span().1);
                    self.set(node, "@after", after);
                    next = b.items.len();
                }
                node
            }
            SK::Break => self.add("break"),
            SK::Continue => self.add("continue"),
            SK::Type => {
                let node = self.add("type");
                let text = self.src[s.lo..s.hi].trim_end_matches([';', ' ', '\t', '\r', '\n']);
                self.word(node, "Text", text);
                node
            }
            SK::Bad => {
                let text = self.src[s.lo..s.hi].trim_end_matches([';', ' ', '\t', '\r', '\n']);
                self.luau(text)
            }
        };
        (node, next)
    }

    fn call_stmt(&mut self, e: &E, s: &S) -> NodeId {
        match &e.k {
            K::Call(f, args, style) => {
                let node = self.add("call");
                self.bind(node, "Function", f);
                self.args(node, args, *style);
                node
            }
            K::Method(o, name, args, style) => {
                let node = self.add("method");
                self.bind(node, "Object", o);
                self.word(node, "Method", name);
                self.args(node, args, *style);
                node
            }
            _ => {
                let text = self.src[s.lo..s.hi].trim_end_matches([';', ' ', '\t', '\r', '\n']);
                self.luau(text)
            }
        }
    }

    fn args(&mut self, node: NodeId, args: &[E], style: Style) {
        self.set(node, "#args", args.len().to_string());
        match style {
            Style::Str => self.set(node, "@style", "string"),
            Style::Table => self.set(node, "@style", "table"),
            Style::Paren => {}
        }
        for (n, e) in args.iter().enumerate() {
            self.bind(node, &format!("Input {}", n + 1), e);
        }
    }

    /// A block hung off `pin` of `node`, with `names` declared inside it.
    fn nested(&mut self, node: NodeId, pin: &str, body: &Block, names: &[String]) {
        self.scopes.push(HashMap::new());
        self.declare_all(names);
        if let Some((head, _)) = self.seq_block(body) {
            self.link(End::new(node, pin), End::new(head, ""));
        }
        self.scopes.pop();
    }

    fn func(&mut self, node: NodeId, f: &F, attrs: &str) {
        if !f.params.is_empty() {
            self.word(node, "Parameters", &f.params);
        }
        for (key, text) in [
            ("@attributes", attrs),
            ("@generics", f.generics.as_str()),
            ("@returns", f.returns.as_str()),
        ] {
            if !text.is_empty() {
                self.set(node, key, text);
            }
        }
        self.nested(node, "Body", &f.body, &f.names);
    }

    // Values.

    /// Fills input `pin` of `node` from `e`.
    fn bind(&mut self, node: NodeId, pin: &str, e: &E) {
        // `E` lives in the tree, which outlives the builder.
        let e: &E = unsafe_extend(e);
        self.bind_tree(node, pin, e);
    }

    fn bind_tree(&mut self, node: NodeId, pin: &str, e: &E) {
        let Some(p) = self.g.input_pin(node, pin) else {
            self.failed = true;
            return;
        };
        match self.expr(e, p.ty, p.default.is_some()) {
            V::Text(text) => self.g.set_value(&End::new(node, pin), text),
            V::End(from) => {
                if self.g.connect(from.clone(), End::new(node, pin)).is_err() {
                    // A typed output (an event's `hit`) into a pin of another
                    // type: read it by name instead.
                    match self.names_of.get(&from).cloned() {
                        Some(name) => {
                            let get = self.add("get");
                            self.word(get, "Name", &name);
                            self.link(End::new(get, "Value"), End::new(node, pin));
                        }
                        None => self.failed = true,
                    }
                }
            }
        }
    }

    /// `e` as a literal typed on a pin of type `ty` when there is one, else
    /// as a node.
    fn expr(&mut self, e: &E, ty: PinType, literal_ok: bool) -> V {
        let lead = self.take(false, e.lo);
        let trail = self.take(true, e.hi);
        let has = !lead.is_empty() || !trail.is_empty();
        if literal_ok && !has {
            if let Some(text) = self.candidate(e, ty) {
                return V::Text(text);
            }
        }
        let end = self.build(e, ty, has);
        if has {
            let (lead, trail) = (self.lead_text(&lead), self.trail_text(&trail));
            if !lead.is_empty() {
                self.set(end.node, "@lead", lead);
            }
            if !trail.is_empty() {
                self.set(end.node, "@trail", trail);
            }
        }
        V::End(end)
    }

    fn candidate(&self, e: &E, ty: PinType) -> Option<String> {
        let text = self.text(e.lo, e.hi);
        let plain = !text.contains(char::is_whitespace);
        let mut options = Vec::new();
        match &e.k {
            K::Lit => {
                if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
                    options.push(text[1..text.len() - 1].to_owned());
                }
                options.insert(0, text.clone());
            }
            K::Un(op, inner) if op == "-" && matches!(inner.k, K::Lit) && plain => {
                options.push(text.clone())
            }
            K::Name(_) | K::Field(..) if plain && self.global_path(e) => options.push(text.clone()),
            _ => {}
        }
        options
            .into_iter()
            .find(|c| codegen::literal(c, ty).is_ok_and(|(t, _)| t == text))
    }

    /// `script`, `game.Players`: a chain of fields on a name nothing declares.
    fn global_path(&self, e: &E) -> bool {
        match &e.k {
            K::Name(n) => {
                matches!(n.as_str(), "script" | "workspace" | "game")
                    && matches!(self.lookup(n), Bind::Plain)
            }
            K::Field(base, _) => self.global_path(base),
            _ => false,
        }
    }

    /// A node for `e`; the output that carries its value.
    fn build(&mut self, e: &E, ty: PinType, has: bool) -> End {
        if self.recog {
            // The pattern's holes borrow from `e`; rolled back on a refusal.
            for hit in pattern::expression(self.src, unsafe_extend(e)) {
                let Some(out) = hit.kind.outputs.iter().find(|p| p.ty != PinType::Exec) else {
                    continue;
                };
                if !ty.accepts(out.ty) {
                    continue;
                }
                let (was, m) = (self.failed, self.mark());
                self.failed = false;
                let node = self.catalog_node(&hit);
                if !self.failed {
                    self.failed = was;
                    return End::new(node, out.name);
                }
                self.rollback(&m);
                self.failed = was;
            }
        }
        self.generic(e, has)
    }

    fn value_node(&mut self, key: &str) -> End {
        End::new(self.add(key), "Value")
    }

    fn generic(&mut self, e: &E, has: bool) -> End {
        match &e.k {
            K::Name(n) => match self.lookup(n) {
                Bind::Wired(end) => {
                    self.read(&end, n);
                    if !has {
                        return end;
                    }
                    // A comment needs a node of its own to hang on.
                    let node = self.value_node("literal");
                    self.word(node.node, "Text", n);
                    self.names_of.insert(node.clone(), n.clone());
                    let _ = end;
                    node
                }
                Bind::Plain => {
                    let node = self.value_node("get");
                    self.word(node.node, "Name", n);
                    node
                }
            },
            K::Lit | K::Bad => {
                let node = self.value_node("literal");
                let text = self.text(e.lo, e.hi);
                self.word(node.node, "Text", &text);
                node
            }
            K::Field(o, n) => {
                let node = self.value_node("field");
                self.bind(node.node, "Object", o);
                self.word(node.node, "Name", n);
                node
            }
            K::Index(o, k) => {
                let node = self.value_node("index");
                self.bind(node.node, "Object", o);
                self.bind(node.node, "Key", k);
                node
            }
            K::Call(f, args, style) => {
                let node = self.value_node("call_value");
                self.bind(node.node, "Function", f);
                self.args(node.node, args, *style);
                node
            }
            K::Method(o, name, args, style) => {
                let node = self.value_node("method_value");
                self.bind(node.node, "Object", o);
                self.word(node.node, "Method", name);
                self.args(node.node, args, *style);
                node
            }
            K::Bin(a, op, b) => {
                let node = self.value_node("binary");
                self.bind(node.node, "A", a);
                self.word(node.node, "Op", op);
                self.bind(node.node, "B", b);
                node
            }
            K::Un(op, a) => {
                let node = self.value_node("unary");
                self.word(node.node, "Op", op);
                self.bind(node.node, "Value", a);
                node
            }
            K::Paren(a) => {
                let node = self.value_node("paren");
                self.bind(node.node, "Value", a);
                node
            }
            K::Func(f) => {
                let node = self.value_node("function_value");
                self.func(node.node, f, &f.attrs);
                node
            }
            K::Table(fields) => self.table(e, fields),
            K::IfExp(arms, els) => {
                let node = self.value_node("if_value");
                self.set(node.node, "#branches", arms.len().to_string());
                for (n, (c, t)) in arms.iter().enumerate() {
                    self.bind(node.node, &format!("Condition {}", n + 1), c);
                    self.bind(node.node, &format!("Then {}", n + 1), t);
                }
                self.bind(node.node, "Else", els);
                node
            }
            K::Interp(segs, es) => {
                let node = self.value_node("interp");
                self.set(node.node, "#parts", es.len().to_string());
                for (n, seg) in segs.iter().enumerate() {
                    if !seg.is_empty() {
                        self.set(node.node, &format!("@seg{n}"), seg.clone());
                    }
                }
                for (n, x) in es.iter().enumerate() {
                    self.bind(node.node, &format!("Value {}", n + 1), x);
                }
                node
            }
            K::Cast(a, ty) => {
                let node = self.value_node("cast");
                self.bind(node.node, "Value", a);
                self.word(node.node, "Type", ty);
                node
            }
        }
    }

    fn table(&mut self, e: &E, fields: &[Fld]) -> End {
        let node = self.value_node("table");
        let id = node.node;
        self.set(id, "#fields", fields.len().to_string());
        if self.src[e.lo..e.hi].contains('\n') {
            self.set(id, "@multiline", "1");
        }
        for (i, f) in fields.iter().enumerate() {
            let n = i + 1;
            let lead = self.take(false, f.lo);
            if !lead.is_empty() {
                let text = lead
                    .iter()
                    .map(|c| self.text(c.lo, c.hi))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.set(id, &format!("@lead{n}"), text);
            }
            let item = format!("Item {n}");
            match &f.key {
                Key::None => self.bind(id, &item, &f.value),
                Key::Name(k) => {
                    self.set(id, &format!("@key{n}"), k.clone());
                    self.bind(id, &item, &f.value);
                }
                Key::Expr(k) => {
                    let pair = self.value_node("pair");
                    self.bind(pair.node, "Key", k);
                    self.bind(pair.node, "Value", &f.value);
                    self.link(pair, End::new(id, &item));
                }
            }
            if !f.sep.is_empty() {
                self.set(id, &format!("@sep{n}"), f.sep.clone());
            }
        }
        let tail = self.take(false, e.hi.saturating_sub(1));
        if !tail.is_empty() {
            let text = tail
                .iter()
                .map(|c| self.text(c.lo, c.hi))
                .collect::<Vec<_>>()
                .join("\n");
            self.set(id, "@tail", text);
        }
        node
    }
}

/// The tree outlives the builder; a pattern hit's borrows need not be
/// tied to the `&mut self` of the call that makes it.
fn unsafe_extend<'x, T>(value: &T) -> &'x T {
    // SAFETY: only ever called with parts of `B::tree`, which is borrowed for
    // longer than any `B` method runs.
    unsafe { &*std::ptr::from_ref(value) }
}
