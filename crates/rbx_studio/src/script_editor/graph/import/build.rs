//! Statements drawn as nodes where a catalog template matches them, and
//! kept as Luau Code nodes where none does. Nothing here has to be right:
//! [`super::import`] compiles the result and checks it against the source,
//! and turns whatever came out different back into raw code.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::super::catalog::{self, Code, Kind, Pin, PinType, Prec};
use super::super::codegen::{indexable, literal, quote};
use super::super::{End, Graph, NodeId};
use super::lex::{unquote, T};
use super::parse::{Shape, Stmt, P};
use super::raw_lines;

/// The names a run can read, and the outputs they stand for.
type Binds = HashMap<String, End>;

/// How much matching one statement may do before it is kept as written.
const BUDGET: usize = 20_000;

pub(super) struct Builder<'a> {
    p: &'a P<'a>,
    comments: &'a [(usize, usize)],
    pinned: &'a HashSet<usize>,
    pub(super) graph: Graph,
    /// Statements drawn as nodes, as token ranges, keyed by first token.
    pub(super) converted: Vec<(usize, usize)>,
    /// Which statement made each node.
    pub(super) owner: HashMap<NodeId, usize>,
    /// The events and start nodes, top to bottom.
    pub(super) items: Vec<NodeId>,
    current: usize,
    steps: usize,
}

struct Snapshot(usize, usize, usize);

#[derive(Clone, Copy)]
enum Ctx {
    /// Written as is, never bracketed (a condition, a loop's list).
    Free,
    /// A template's hole: bracketed by the template's rules.
    Hole { parent: Prec, indexed: bool },
}

#[derive(Clone, Copy)]
enum Bind {
    Span(usize, usize),
    Dot(usize),
    Bracket(usize, usize),
}

enum Piece {
    Toks(Vec<(T, &'static str)>),
    Hole { pin: &'static str, indexed: bool },
    Index(&'static str),
}

/// What a converted statement leaves the run with.
enum Done {
    /// A node, and the output its run carries on from.
    Node(NodeId, &'static str),
    Local(String, End),
}

impl<'a> Builder<'a> {
    pub(super) fn new(
        p: &'a P<'a>,
        comments: &'a [(usize, usize)],
        pinned: &'a HashSet<usize>,
    ) -> Self {
        Builder {
            p,
            comments,
            pinned,
            graph: Graph::default(),
            converted: Vec::new(),
            owner: HashMap::new(),
            items: Vec::new(),
            current: 0,
            steps: BUDGET,
        }
    }

    fn snap(&self) -> Snapshot {
        Snapshot(
            self.graph.nodes.len(),
            self.graph.wires.len(),
            self.converted.len(),
        )
    }

    fn rollback(&mut self, Snapshot(nodes, wires, converted): Snapshot) {
        self.graph.nodes.truncate(nodes);
        self.graph.wires.truncate(wires);
        self.converted.truncate(converted);
    }

    fn add(&mut self, kind: &Kind) -> NodeId {
        let id = self.graph.add(kind, [0.0, 0.0]);
        self.owner.insert(id, self.current);
        id
    }

    fn has_comment(&self, lo: usize, hi: usize) -> bool {
        self.comments
            .iter()
            .any(|&(start, _)| start >= lo && start < hi)
    }

    fn start(&self, i: usize) -> usize {
        self.p.toks[i].start
    }

    fn end(&self, i: usize) -> usize {
        self.p.toks[i].end
    }

    /// The whole text: top-level statements between events go under a
    /// start node, in order.
    pub(super) fn top(&mut self, stmts: &[Stmt], lo: usize, hi: usize) {
        let start = catalog::kind("start").expect("the catalog has a start event");
        let (mut seg, mut seg_lo) = (0, lo);
        let segment = |this: &mut Self, from: usize, to: usize, lo: usize, hi: usize| {
            if from == to && !this.has_comment(lo, hi) {
                return;
            }
            this.current = stmts.get(from).map_or(0, |stmt| stmt.lo);
            let node = this.add(start);
            this.run(
                &stmts[from..to],
                lo,
                hi,
                End::new(node, ""),
                &mut Binds::new(),
            );
            this.items.push(node);
        };
        for (n, stmt) in stmts.iter().enumerate() {
            if !matches!(stmt.shape, Shape::Plain) || self.pinned.contains(&stmt.lo) {
                continue;
            }
            let snap = self.snap();
            let items = self.items.len();
            let at = self.start(stmt.lo);
            segment(self, seg, n, seg_lo, at);
            match self.event(stmt) {
                Some(event) => {
                    self.items.push(event);
                    seg = n + 1;
                    seg_lo = self.end(stmt.hi - 1);
                }
                None => {
                    self.rollback(snap);
                    self.items.truncate(items);
                }
            }
        }
        segment(self, seg, stmts.len(), seg_lo, hi);
        for (y, id) in self.items.clone().into_iter().enumerate() {
            if let Some(node) = self.graph.node_mut(id) {
                node.y = y as f32;
            }
        }
    }

    /// `SIGNAL:Connect(function(params) BODY end)` for a catalog event.
    fn event(&mut self, stmt: &Stmt) -> Option<NodeId> {
        let p = self.p;
        let (lo, hi) = (stmt.lo, stmt.hi);
        if hi - lo < 9 || !p.is(hi - 1, ")") || !p.is(hi - 2, "end") {
            return None;
        }
        let mut depth = 0i32;
        let mut colon = None;
        for i in lo..hi - 4 {
            match p.text(i) {
                "(" | "[" | "{" if p.toks[i].t == T::Op => depth += 1,
                ")" | "]" | "}" if p.toks[i].t == T::Op => depth -= 1,
                ":" if depth == 0
                    && p.toks[i].t == T::Op
                    && p.text(i + 1) == "Connect"
                    && p.toks[i + 1].t == T::Name
                    && p.is(i + 2, "(")
                    && p.is(i + 3, "function")
                    && p.is(i + 4, "(") =>
                {
                    colon = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let colon = colon?;
        let mut params = Vec::new();
        let mut j = colon + 5;
        while !p.is(j, ")") {
            if p.toks.get(j)?.t != T::Name {
                return None;
            }
            params.push(p.text(j).to_owned());
            j += 1;
            if p.is(j, ",") {
                j += 1;
            }
        }
        let (body, end) = p.block(j + 1, &["end"], self.end(j))?;
        if end != hi - 2
            || self.has_comment(self.start(lo), body.lo)
            || self.has_comment(body.hi, self.end(hi - 1))
        {
            return None;
        }
        self.current = lo;
        self.steps = BUDGET;
        for kind in catalog::all() {
            let Code::Event(Some(signal)) = kind.code else {
                continue;
            };
            let outputs: Vec<&Pin> = kind
                .outputs
                .iter()
                .filter(|pin| pin.ty != PinType::Exec)
                .collect();
            if outputs.len() != params.len() {
                continue;
            }
            for found in self.matches(pieces(signal), lo, colon) {
                let snap = self.snap();
                let node = self.add(kind);
                if !self.apply(node, kind, signal, &found, Prec::Atom, &Binds::new()) {
                    self.rollback(snap);
                    continue;
                }
                let mut binds: Binds = params
                    .iter()
                    .zip(&outputs)
                    .map(|(name, pin)| (name.clone(), End::new(node, pin.name)))
                    .collect();
                self.run(
                    &body.stmts,
                    body.lo,
                    body.hi,
                    End::new(node, ""),
                    &mut binds,
                );
                self.converted.push((lo, hi));
                return Some(node);
            }
        }
        None
    }

    /// A run of statements from `from`: each drawn as nodes where it can
    /// be, the rest gathered into Luau Code nodes. The run's last output.
    fn run(
        &mut self,
        stmts: &[Stmt],
        lo: usize,
        hi: usize,
        mut from: End,
        binds: &mut Binds,
    ) -> End {
        let mut raw: Option<(usize, usize)> = None;
        let mut cursor = lo;
        let comments = self.comments;
        let keep = |raw: &mut Option<(usize, usize)>, a: usize, b: usize| {
            *raw = Some(raw.map_or((a, b), |(start, _)| (start, b)));
        };
        for stmt in stmts {
            let (a, b) = (self.start(stmt.lo), self.end(stmt.hi - 1));
            for &(c, d) in comments.iter().filter(|(c, _)| *c >= cursor && *c < a) {
                keep(&mut raw, c, d);
            }
            cursor = b;
            if !self.pinned.contains(&stmt.lo) {
                let snap = self.snap();
                self.current = stmt.lo;
                self.steps = BUDGET;
                match self.stmt(stmt, binds) {
                    Some(Done::Node(node, next)) => {
                        from = self.flush(&mut raw, from);
                        let _ = self.graph.connect(from, End::new(node, ""));
                        from = End::new(node, next);
                        self.converted.push((stmt.lo, stmt.hi));
                        continue;
                    }
                    Some(Done::Local(name, end)) => {
                        from = self.flush(&mut raw, from);
                        binds.insert(name, end);
                        self.converted.push((stmt.lo, stmt.hi));
                        continue;
                    }
                    None => self.rollback(snap),
                }
            }
            keep(&mut raw, a, b);
        }
        for &(c, d) in comments.iter().filter(|(c, _)| *c >= cursor && *c < hi) {
            keep(&mut raw, c, d);
        }
        self.flush(&mut raw, from)
    }

    /// The gathered raw text as one Luau Code node after `from`.
    fn flush(&mut self, raw: &mut Option<(usize, usize)>, from: End) -> End {
        let Some((a, b)) = raw.take() else {
            return from;
        };
        let src = self.p.src;
        let line = src[..a].rfind('\n').map_or(0, |n| n + 1);
        let prefix = Some(&src[line..a])
            .filter(|text| text.trim().is_empty())
            .unwrap_or("");
        let mut code = String::new();
        for (n, (text, verbatim)) in raw_lines(&src[a..b]).into_iter().enumerate() {
            if n > 0 {
                code.push('\n');
            }
            match n == 0 || verbatim {
                true => code.push_str(text),
                false => code.push_str(text.strip_prefix(prefix).unwrap_or(text.trim_start())),
            }
        }
        let kind = catalog::kind("luau").expect("the catalog has a Luau Code node");
        let node = self.add(kind);
        self.graph.set_value(&End::new(node, "Code"), code);
        let _ = self.graph.connect(from, End::new(node, ""));
        End::new(node, "")
    }

    fn stmt(&mut self, stmt: &Stmt, binds: &Binds) -> Option<Done> {
        let p = self.p;
        let (lo, hi) = (stmt.lo, stmt.hi);
        match &stmt.shape {
            Shape::Plain => {
                if self.has_comment(self.start(lo), self.end(hi - 1)) {
                    return None;
                }
                if p.is(lo, "local") {
                    if hi < lo + 4 || p.toks[lo + 1].t != T::Name || !p.is(lo + 2, "=") {
                        return None;
                    }
                    let (end, _) = self.value(lo + 3, hi, PinType::Any, binds, false)?;
                    return Some(Done::Local(p.text(lo + 1).to_owned(), end));
                }
                for kind in catalog::all() {
                    let Code::Statement(template) = kind.code else {
                        continue;
                    };
                    for found in self.matches(pieces(template), lo, hi) {
                        let snap = self.snap();
                        let node = self.add(kind);
                        if self.apply(node, kind, template, &found, Prec::Atom, binds) {
                            return Some(Done::Node(node, ""));
                        }
                        self.rollback(snap);
                    }
                }
                None
            }
            Shape::If { cond, yes, no } => {
                let last = no.as_ref().map_or(yes.hi, |no| no.hi);
                if self.has_comment(self.start(lo), yes.lo)
                    || self.has_comment(last, self.end(hi - 1))
                {
                    return None;
                }
                let kind = catalog::kind("branch")?;
                let node = self.add(kind);
                if !self.feed(
                    node,
                    kind.input("Condition")?,
                    cond.0,
                    cond.1,
                    Ctx::Free,
                    binds,
                ) {
                    return None;
                }
                self.run(
                    &yes.stmts,
                    yes.lo,
                    yes.hi,
                    End::new(node, "True"),
                    &mut binds.clone(),
                );
                if let Some(no) = no {
                    self.run(
                        &no.stmts,
                        no.lo,
                        no.hi,
                        End::new(node, "False"),
                        &mut binds.clone(),
                    );
                }
                Some(Done::Node(node, "Completed"))
            }
            Shape::For { head: (a, b), body } => {
                if self.has_comment(self.start(lo), body.lo)
                    || self.has_comment(body.hi, self.end(hi - 1))
                {
                    return None;
                }
                let name = |i: usize| (p.toks[i].t == T::Name).then(|| p.text(i).to_owned());
                let mut inner = binds.clone();
                let node = if b - a >= 5 && p.is(a + 1, ",") && p.is(a + 3, "in") {
                    let (index, item) = (name(*a)?, name(a + 2)?);
                    let kind = catalog::kind("for_each_item")?;
                    let node = self.add(kind);
                    if !self.feed(node, kind.input("List")?, a + 4, *b, Ctx::Free, binds) {
                        return None;
                    }
                    inner.insert(index, End::new(node, "Index"));
                    inner.insert(item, End::new(node, "Item"));
                    node
                } else if b - a >= 5 && p.is(a + 1, "=") && p.text(a + 2) == "1" && p.is(a + 3, ",")
                {
                    let index = name(*a)?;
                    let kind = catalog::kind("repeat_times")?;
                    let node = self.add(kind);
                    if !self.feed(node, kind.input("Count")?, a + 4, *b, Ctx::Free, binds) {
                        return None;
                    }
                    inner.insert(index, End::new(node, "Index"));
                    node
                } else {
                    return None;
                };
                self.run(
                    &body.stmts,
                    body.lo,
                    body.hi,
                    End::new(node, "Loop"),
                    &mut inner,
                );
                Some(Done::Node(node, "Completed"))
            }
            Shape::Other => None,
        }
    }

    /// Fills `node`'s holes from what a template match found.
    fn apply(
        &mut self,
        node: NodeId,
        kind: &Kind,
        template: &'static str,
        found: &[Bind],
        parent: Prec,
        binds: &Binds,
    ) -> bool {
        let mut found = found.iter();
        for piece in pieces(template) {
            let (pin, ctx) = match piece {
                Piece::Toks(_) => continue,
                Piece::Hole { pin, indexed } => (
                    *pin,
                    Ctx::Hole {
                        parent,
                        indexed: *indexed,
                    },
                ),
                Piece::Index(pin) => (*pin, Ctx::Free),
            };
            let Some(input) = kind.input(pin) else {
                return false;
            };
            let ok = match found.next() {
                Some(Bind::Dot(i)) => {
                    self.graph
                        .set_value(&End::new(node, pin), self.p.text(*i).to_owned());
                    true
                }
                Some(Bind::Span(a, b) | Bind::Bracket(a, b)) => {
                    self.feed(node, input, *a, *b, ctx, binds)
                }
                None => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// Puts the expression in tokens `a..b` into `node`'s `pin`: as a wire
    /// from a bound name, a typed literal, or a value node.
    fn feed(
        &mut self,
        node: NodeId,
        pin: &Pin,
        a: usize,
        b: usize,
        ctx: Ctx,
        binds: &Binds,
    ) -> bool {
        let p = self.p;
        if a >= b {
            return false;
        }
        let paren = p.is(a, "(") && p.close(a) == Some(b - 1);
        let (a, b) = match paren {
            true => (a + 1, b - 1),
            false => (a, b),
        };
        if a >= b {
            return false;
        }
        let first = &p.src[self.start(a)..self.end(b - 1)];
        let to = End::new(node, pin.name);
        if b == a + 1 && p.toks[a].t == T::Name {
            if let Some(end) = binds.get(p.text(a)) {
                return !paren && self.graph.connect(end.clone(), to).is_ok();
            }
        }
        if pin.default.is_some() {
            if let Some(value) = self.lit_for(a, b, pin.ty, binds) {
                if let Ok((text, prec)) = literal(&value, pin.ty) {
                    if needs(ctx, prec, &text) == paren {
                        self.graph.set_value(&to, value);
                        return true;
                    }
                }
            }
        }
        let snap = self.snap();
        if let Some((end, prec)) = self.value(a, b, pin.ty, binds, true) {
            if needs(ctx, prec, first) == paren && self.graph.connect(end, to).is_ok() {
                return true;
            }
            self.rollback(snap);
        }
        false
    }

    /// The text of tokens `a..b` as a literal of type `ty`, if writing it
    /// back as one gives the same Luau.
    fn lit_for(&self, a: usize, b: usize, ty: PinType, binds: &Binds) -> Option<String> {
        let p = self.p;
        if b == a + 1 && p.toks[a].t == T::Str {
            let value = unquote(p.text(a))?;
            return match ty {
                PinType::String => Some(value),
                PinType::Any => {
                    let quoted = quote(&value);
                    (literal(&quoted, ty).ok()?.0 == quoted).then_some(quoted)
                }
                _ => None,
            };
        }
        if p.toks[a..b]
            .iter()
            .any(|tok| matches!(tok.t, T::Str | T::Interp))
            || (p.toks[a].t == T::Name && binds.contains_key(p.text(a)))
        {
            return None;
        }
        let text = &p.src[self.start(a)..self.end(b - 1)];
        (literal(text, ty).ok()?.0 == text).then(|| text.to_owned())
    }

    /// A value node (or a bound name) for tokens `a..b`, giving `ty`, and
    /// the precedence the compiler writes it at. `inline` skips the calls
    /// the compiler always reads into a local first.
    fn value(
        &mut self,
        a: usize,
        b: usize,
        ty: PinType,
        binds: &Binds,
        inline: bool,
    ) -> Option<(End, Prec)> {
        let p = self.p;
        if b == a + 1 && p.toks[a].t == T::Name {
            if let Some(end) = binds.get(p.text(a)) {
                return Some((end.clone(), Prec::Atom));
            }
        }
        for kind in catalog::all() {
            let Code::Expression {
                template,
                prec,
                call,
            } = kind.code
            else {
                continue;
            };
            let Some(out) = kind.outputs.iter().find(|pin| pin.ty != PinType::Exec) else {
                continue;
            };
            if (inline && call) || !ty.accepts(out.ty) {
                continue;
            }
            if let [Piece::Hole { pin, .. }] = pieces(template) {
                let Some(input) = kind.input(pin) else {
                    continue;
                };
                if let Some(value) = self.lit_for(a, b, input.ty, binds) {
                    let node = self.add(kind);
                    self.graph.set_value(&End::new(node, pin), value);
                    return Some((End::new(node, out.name), prec));
                }
                continue;
            }
            for found in self.matches(pieces(template), a, b) {
                let snap = self.snap();
                let node = self.add(kind);
                if self.apply(node, kind, template, &found, prec, binds) {
                    return Some((End::new(node, out.name), prec));
                }
                self.rollback(snap);
            }
        }
        None
    }

    /// Every way (up to 16) the template's pieces cover tokens `a..b`.
    fn matches(&mut self, pieces: &[Piece], a: usize, b: usize) -> Vec<Vec<Bind>> {
        let mut out = Vec::new();
        self.find(pieces, a, b, &mut Vec::new(), &mut out);
        out
    }

    fn find(
        &mut self,
        pieces: &[Piece],
        pos: usize,
        b: usize,
        acc: &mut Vec<Bind>,
        out: &mut Vec<Vec<Bind>>,
    ) {
        if out.len() >= 16 || self.steps == 0 {
            return;
        }
        self.steps -= 1;
        let p = self.p;
        let Some((piece, rest)) = pieces.split_first() else {
            if pos == b {
                out.push(acc.clone());
            }
            return;
        };
        match piece {
            Piece::Toks(toks) => {
                if pos + toks.len() <= b
                    && toks
                        .iter()
                        .enumerate()
                        .all(|(k, &(t, s))| self.tok_eq(pos + k, t, s))
                {
                    self.find(rest, pos + toks.len(), b, acc, out);
                }
            }
            Piece::Index(_) => {
                if p.is(pos, ".") && pos + 1 < b && p.toks[pos + 1].t == T::Name {
                    acc.push(Bind::Dot(pos + 1));
                    self.find(rest, pos + 2, b, acc, out);
                    acc.pop();
                }
                if p.is(pos, "[") {
                    if let Some(close) = p.close(pos).filter(|&close| close < b) {
                        acc.push(Bind::Bracket(pos + 1, close));
                        self.find(rest, close + 1, b, acc, out);
                        acc.pop();
                    }
                }
            }
            Piece::Hole { .. } => {
                let is_if = p.is(pos, "if");
                let mut depth = 0usize;
                for e in pos..b {
                    let tok = &p.toks[e];
                    let text = p.text(e);
                    match tok.t {
                        T::Op => match text {
                            "(" | "[" | "{" => depth += 1,
                            ")" | "]" | "}" => match depth.checked_sub(1) {
                                Some(d) => depth = d,
                                None => return,
                            },
                            "," | "=" | ";" if depth == 0 => return,
                            _ => {}
                        },
                        T::Keyword => match text {
                            "then" | "else" | "elseif" if is_if => {}
                            "and" | "or" | "not" | "nil" | "true" | "false" | "if" => {}
                            _ => return,
                        },
                        _ => {}
                    }
                    if depth != 0 {
                        continue;
                    }
                    let next = e + 1;
                    let fits = match rest.first() {
                        None => next == b,
                        Some(Piece::Toks(toks)) => {
                            toks.first().is_some_and(|&(t, s)| self.tok_eq(next, t, s))
                        }
                        Some(Piece::Index(_)) => p.is(next, ".") || p.is(next, "["),
                        Some(Piece::Hole { .. }) => true,
                    };
                    if fits {
                        acc.push(Bind::Span(pos, next));
                        self.find(rest, next, b, acc, out);
                        acc.pop();
                        if out.len() >= 16 || self.steps == 0 {
                            return;
                        }
                    }
                }
            }
        }
    }

    fn tok_eq(&self, i: usize, t: T, s: &str) -> bool {
        let p = self.p;
        let Some(tok) = p.toks.get(i) else {
            return false;
        };
        tok.t == t
            && match t {
                T::Str => {
                    p.text(i) == s || unquote(p.text(i)).is_some_and(|u| Some(u) == unquote(s))
                }
                _ => p.text(i) == s,
            }
    }
}

/// Whether the compiler brackets a value of `prec` written as `text`.
fn needs(ctx: Ctx, prec: Prec, text: &str) -> bool {
    match ctx {
        Ctx::Free => false,
        Ctx::Hole { indexed: true, .. } => prec != Prec::Atom || !indexable(text),
        Ctx::Hole { parent, .. } => parent != Prec::Atom && prec != Prec::Atom && prec <= parent,
    }
}

/// A template split into its fixed tokens and its holes, as the compiler
/// fills them.
fn pieces(template: &'static str) -> &'static [Piece] {
    static ALL: OnceLock<HashMap<&'static str, Vec<Piece>>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        catalog::all()
            .filter_map(|kind| match kind.code {
                Code::Event(Some(t))
                | Code::Statement(t)
                | Code::Expression { template: t, .. } => Some(t),
                _ => None,
            })
            .map(|template| (template, split(template)))
            .collect()
    });
    all.get(template).map_or(&[], Vec::as_slice)
}

fn split(template: &'static str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut rest = template;
    let fixed = |pieces: &mut Vec<Piece>, text: &'static str| {
        if text.trim().is_empty() {
            return;
        }
        let toks = super::lex::lex(text).map_or(Vec::new(), |lexed| {
            lexed
                .toks
                .iter()
                .map(|tok| (tok.t, &text[tok.start..tok.end]))
                .collect()
        });
        pieces.push(Piece::Toks(toks));
    };
    while let Some(open) = rest.find('{') {
        fixed(&mut pieces, &rest[..open]);
        let close = open + rest[open..].find('}').expect("templates close every hole");
        let hole = &rest[open + 1..close];
        rest = &rest[close + 1..];
        match hole.strip_prefix('.') {
            Some(pin) => pieces.push(Piece::Index(pin)),
            None => {
                let indexed = rest.starts_with(['.', ':', '[', '(']) || rest.starts_with("{.");
                pieces.push(Piece::Hole { pin: hole, indexed });
            }
        }
    }
    fixed(&mut pieces, rest);
    pieces
}
