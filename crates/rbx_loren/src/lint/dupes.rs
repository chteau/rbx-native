//! Scripts that index an instance by a name two siblings share.
//!
//! Measured in Studio's command bar with two children of Workspace both named
//! "Part": `workspace.Part`, `workspace["Part"]`, `workspace:FindFirstChild("Part")`
//! and `workspace:WaitForChild("Part")` all return the first of the two in
//! `GetChildren()` order, and after re-parenting that first part
//! (`Parent = nil`, then back to Workspace) `workspace.Part` returns the other
//! one. Which part a script gets depends on child order, which shifts whenever
//! something is re-parented.
//!
//! The lint walks each script's index chains against the place tree. A step
//! that finds no child stops silently (a script may create it at runtime), one
//! child carries on, two or more are reported and end that chain.

use std::ops::Range;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::scan::Severity;
use super::tokenize::{structure, tokenize, Token, TokenKind};

mod members;

pub use members::is_class_member;

pub const KIND: &str = "duplicate-name";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupeIssue {
    pub script: Ref,
    /// 1-based.
    pub line: usize,
    /// The shared name inside the script's `Source`, without quotes.
    pub range: Range<usize>,
    pub kind: &'static str,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Copy)]
enum Node {
    /// `game`: the place's top-level services.
    Game,
    Inst(Ref),
}

enum Step<'a> {
    Child(&'a str),
    Parent,
    Service(&'a str),
}

/// Every duplicate-name use in the place's scripts. `is_member(class, name)`
/// says whether `name` is a property, method or event of `class`, which wins
/// over a child of the same name.
pub fn duplicate_names(dom: &WeakDom, is_member: &dyn Fn(&str, &str) -> bool) -> Vec<DupeIssue> {
    let db = ReflectionDatabase::shared();
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get(r) else { continue };
        stack.extend(inst.children());
        if !db.is_subclass_of(inst.class(), "LuaSourceContainer") {
            continue;
        }
        if let Some(Variant::String(src)) = inst.properties().get("Source") {
            let walker = Walker {
                dom,
                is_member,
                script: r,
                src,
                toks: tokenize(src),
            };
            out.extend(walker.run());
        }
    }
    out.sort_by_key(|i| (i.script, i.range.start));
    out
}

struct Walker<'a> {
    dom: &'a WeakDom,
    is_member: &'a dyn Fn(&str, &str) -> bool,
    script: Ref,
    src: &'a str,
    toks: Vec<Token>,
}

impl Walker<'_> {
    fn run(&self) -> Vec<DupeIssue> {
        let matching = structure(&self.toks).matching;
        let mut issues = Vec::new();
        let mut aliases: Vec<(&str, Node)> = Vec::new();
        let mut i = 0;
        while i < self.toks.len() {
            let t = &self.toks[i];
            let alias_of = (self.is_kw(i, "local")
                && self.kind_at(i + 1) == Some(TokenKind::Name)
                && self.is_op(i + 2, "="))
            .then(|| self.toks[i + 1].value.as_str());
            let start = if alias_of.is_some() { i + 3 } else { i };
            let after_member = i > 0 && (self.is_op(i - 1, ".") || self.is_op(i - 1, ":"));
            let rooted = alias_of.is_some() || (t.kind == TokenKind::Name && !after_member);
            let Some(root) = rooted.then(|| self.root(start, &aliases)).flatten() else {
                i += 1;
                continue;
            };
            let (node, end) = self.chain(start, root, &matching, &mut issues);
            if let (Some(name), Some(node)) = (alias_of, node) {
                // Only a name set once is an alias; a later `x = ...` would make it a guess.
                if self.set_once(name) && self.ends_statement(end) {
                    aliases.push((name, node));
                }
            }
            i = end.max(i + 1);
        }
        issues
    }

    fn kind_at(&self, i: usize) -> Option<TokenKind> {
        self.toks.get(i).map(|t| t.kind)
    }

    fn is_op(&self, i: usize, op: &str) -> bool {
        self.toks
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Op && t.value == op)
    }

    fn is_kw(&self, i: usize, kw: &str) -> bool {
        self.toks
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Kw && t.value == kw)
    }

    /// `local x = ...` is the only place `x` is declared or assigned.
    fn set_once(&self, name: &str) -> bool {
        let mut sets = 0;
        for (i, t) in self.toks.iter().enumerate() {
            if t.kind != TokenKind::Name || t.value != name {
                continue;
            }
            let member = i > 0 && (self.is_op(i - 1, ".") || self.is_op(i - 1, ":"));
            let declared = i > 0
                && (self.is_kw(i - 1, "for")
                    || self.is_kw(i - 1, "function")
                    || self.is_kw(i - 1, "local"));
            if !member && (self.is_op(i + 1, "=") || declared) {
                sets += 1;
            }
        }
        sets == 1
    }

    /// The chain is the whole right-hand side: the next token starts a new line.
    fn ends_statement(&self, end: usize) -> bool {
        match (self.toks.get(end), self.toks.get(end.wrapping_sub(1))) {
            (Some(next), Some(last)) => next.line > last.line,
            _ => true,
        }
    }

    fn root(&self, i: usize, aliases: &[(&str, Node)]) -> Option<Node> {
        let t = self.toks.get(i)?;
        if t.kind != TokenKind::Name {
            return None;
        }
        if let Some((_, node)) = aliases.iter().rev().find(|(n, _)| *n == t.value) {
            return Some(*node);
        }
        match t.value.as_str() {
            "game" => Some(Node::Game),
            "script" => Some(Node::Inst(self.script)),
            "workspace" | "Workspace" => self.service("Workspace").map(Node::Inst),
            _ => None,
        }
    }

    fn service(&self, class: &str) -> Option<Ref> {
        self.dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| self.dom.get(*r).is_some_and(|i| i.class() == class))
    }

    fn children(&self, node: Node) -> &[Ref] {
        match node {
            Node::Game => self.dom.root_refs(),
            Node::Inst(r) => self.dom.get(r).map_or(&[], |i| i.children()),
        }
    }

    fn named(&self, node: Node, name: &str) -> Vec<Ref> {
        self.children(node)
            .iter()
            .copied()
            .filter(|c| self.dom.get(*c).is_some_and(|i| i.name() == name))
            .collect()
    }

    fn label(&self, node: Node) -> String {
        match node {
            Node::Game => "game".to_string(),
            Node::Inst(r) => self
                .dom
                .get(r)
                .map_or_else(String::new, |i| i.name().into()),
        }
    }

    /// The step starting at token `i`, with the index of its name token and of
    /// the token after it. `None` for anything the lint cannot follow, or a
    /// lookup that searches descendants.
    fn step(&self, i: usize, matching: &[i32]) -> Option<(Step<'_>, usize, usize)> {
        let tok = |k: usize| self.toks.get(k);
        let text = |k: usize| {
            tok(k)
                .filter(|t| t.value.starts_with(['"', '\'']))
                .and_then(|t| t.text.as_deref())
        };
        if self.is_op(i, ".") {
            let t = tok(i + 1).filter(|t| t.kind == TokenKind::Name)?;
            let step = if t.value == "Parent" {
                Step::Parent
            } else {
                Step::Child(&t.value)
            };
            return Some((step, i + 1, i + 2));
        }
        if self.is_op(i, "[") {
            let name = text(i + 1).filter(|_| self.is_op(i + 2, "]"))?;
            return Some((Step::Child(name), i + 1, i + 3));
        }
        if !self.is_op(i, ":") || !self.is_op(i + 2, "(") {
            return None;
        }
        let method = tok(i + 1)?.value.as_str();
        let close = usize::try_from(*matching.get(i + 2)?).ok()?;
        let name = text(i + 3)?;
        let rest = close - (i + 4);
        let after_name = |k: usize| tok(i + 4 + k).map(|t| t.value.as_str());
        match method {
            "GetService" if rest == 0 => Some((Step::Service(name), i + 3, close + 1)),
            "WaitForChild" if rest == 0 || after_name(0) == Some(",") => {
                Some((Step::Child(name), i + 3, close + 1))
            }
            // The second argument asks for a descendant search: any depth, nothing to compare.
            "FindFirstChild" if rest == 0 || (rest == 2 && after_name(1) == Some("false")) => {
                Some((Step::Child(name), i + 3, close + 1))
            }
            _ => None,
        }
    }

    /// Follows the chain rooted at token `start`. Returns where it resolved
    /// to (`None` once it left what the lint can follow) and the next token.
    fn chain(
        &self,
        start: usize,
        root: Node,
        matching: &[i32],
        issues: &mut Vec<DupeIssue>,
    ) -> (Option<Node>, usize) {
        let mut node = root;
        let mut i = start + 1;
        while let Some((step, name_tok, next)) = self.step(i, matching) {
            let found = match step {
                Step::Parent => match node {
                    Node::Game => return (None, i),
                    Node::Inst(r) => Some(self.dom.parent(r).map_or(Node::Game, Node::Inst)),
                },
                Step::Service(name) => self.service(name).map(Node::Inst),
                Step::Child(name) => {
                    let method = self.is_op(i, ":");
                    let member = !method
                        && matches!(node, Node::Inst(r)
                            if self.dom.get(r).is_some_and(|n| (self.is_member)(n.class(), name)));
                    if member {
                        return (None, next);
                    }
                    let same = self.named(node, name);
                    if same.len() > 1 {
                        issues.push(self.issue(start, name_tok, next, node, same.len(), name));
                        return (None, next);
                    }
                    same.first().map(|r| Node::Inst(*r))
                }
            };
            let Some(found) = found else {
                return (None, next);
            };
            node = found;
            i = next;
        }
        (Some(node), i)
    }

    fn issue(
        &self,
        start: usize,
        name_tok: usize,
        next: usize,
        owner: Node,
        count: usize,
        name: &str,
    ) -> DupeIssue {
        let first = &self.toks[start];
        let t = &self.toks[name_tok];
        let range = if t.kind == TokenKind::Str {
            t.start + 1..t.end - 1
        } else {
            t.start..t.end
        };
        let chain = &self.src[first.start..self.toks[next - 1].end];
        DupeIssue {
            script: self.script,
            line: t.line,
            range,
            kind: KIND,
            severity: Severity::Warn,
            message: format!(
                "{chain}: {} has {count} children named \"{name}\", and which one this gets depends on child order (it changes when one is re-parented). Rename one so the name is unique.",
                self.label(owner)
            ),
        }
    }
}

#[cfg(test)]
mod tests;
