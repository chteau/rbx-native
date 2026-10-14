//! Debugger hooks for scripts that run in a Play session.
//!
//! A game script cannot read its own locals (Roblox's `debug` library has
//! `info` and `traceback`, no `getlocal`), and nothing outside the game can
//! stop one. So the published copy of a script with breakpoints is
//! rewritten: before the first statement that starts on each line, on that
//! same line (line numbers, and so error messages and `debug.info`, stay
//! exactly as the developer wrote them), it gets
//!
//! ```lua
//! if __rbxdbg_B[N] or __rbxdbg_D.s then __rbxdbg_H(ID, N, "a b", {a, b}, COND, LOG, CONT) end
//! ```
//!
//! - `ID` is the script's id, `N` the 1-based line.
//! - `"a b"` names the locals in scope there (space-separated, in
//!   declaration order, shadowed names once, at most `MAX_LOCALS` of the
//!   innermost), and `{a, b}` holds their values in the same order (a nil
//!   leaves a hole, which is why the names travel separately). No locals:
//!   `""` and `{}`.
//! - `COND`, `LOG`, `CONT` are `nil, nil, nil` except on a line with a
//!   [`Hook`]: `COND` is `function() return (<condition>) end` or `nil`,
//!   `LOG` is `function() print(<log_message>) end` or `nil`, `CONT` is
//!   `true` or `false`.
//!
//! The probe's `__rbxnative_debug` module (`probe/debug.luau`) is the other
//! half. The script gets, before its first statement and on that
//! statement's line (after any `--!` directive lines, which must stay
//! first),
//!
//! ```lua
//! local __rbxdbg_D = require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_debug")) local __rbxdbg_B = __rbxdbg_D.lines(ID, {SERVER LINES}, {CLIENT LINES}) local __rbxdbg_H = __rbxdbg_D.hit
//! ```
//!
//! where the two line lists are the breakpoints active from the start on
//! each side (code that runs at startup stops too, before the editor's
//! first `DebugSet` arrives).

use full_moon::ast::{Block, FunctionBody, Parameter, Stmt};
use full_moon::node::Node;
use full_moon::visitors::{Visit, Visitor};
use std::collections::BTreeMap;

/// The most locals one hook captures (the innermost ones). Every captured
/// local is an upvalue of each nested function that has hooks, and Luau
/// allows 200 upvalues per function.
pub const MAX_LOCALS: usize = 60;

/// A breakpoint known when the script is instrumented.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hook {
    /// 1-based; a line no statement starts on moves to the next hooked line
    /// (`resolve_line`).
    pub line: u32,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub continue_execution: bool,
    /// Active from the start on the server / on the client.
    pub server: bool,
    pub client: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrumented {
    pub source: String,
    /// Every hooked line, ascending: where a breakpoint can stop.
    pub lines: Vec<u32>,
}

/// Rewrites `source` as the module docs describe. `Err` with the parser's
/// message when `source` does not parse (the caller publishes it as is);
/// a source without statements comes back unchanged with no lines.
pub fn instrument(source: &str, script_id: u32, hooks: &[Hook]) -> Result<Instrumented, String> {
    let ast = full_moon::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let mut walker = Walker::default();
    let block = ast.nodes();
    walker.block(block, Vec::new());
    let Some(first) = block
        .stmts()
        .next()
        .and_then(Node::start_position)
        .or_else(|| block.last_stmt().and_then(Node::start_position))
        .map(|p| p.bytes())
    else {
        return Ok(Instrumented {
            source: source.to_owned(),
            lines: Vec::new(),
        });
    };
    let lines: Vec<u32> = walker.sites.keys().copied().collect();

    let mut server = Vec::new();
    let mut client = Vec::new();
    let mut extras: BTreeMap<u32, &Hook> = BTreeMap::new();
    for hook in hooks {
        let Some(line) = resolve_line(&lines, hook.line) else {
            continue;
        };
        if hook.server && !server.contains(&line) {
            server.push(line);
        }
        if hook.client && !client.contains(&line) {
            client.push(line);
        }
        extras.insert(line, hook);
    }
    server.sort_unstable();
    client.sort_unstable();

    let mut inserts: Vec<(usize, String)> = Vec::with_capacity(walker.sites.len() + 1);
    inserts.push((
        first,
        format!(
            "local __rbxdbg_D = require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_debug\")) local __rbxdbg_B = __rbxdbg_D.lines({script_id}, {{{}}}, {{{}}}) local __rbxdbg_H = __rbxdbg_D.hit ",
            join(&server),
            join(&client)
        ),
    ));
    for (line, (offset, locals)) in &walker.sites {
        let tail = match extras.get(line) {
            Some(hook) => format!(
                "{}, {}, {}",
                hook.condition
                    .as_deref()
                    .map_or("nil".to_owned(), |c| format!("function() return ({c}) end")),
                hook.log_message
                    .as_deref()
                    .map_or("nil".to_owned(), |m| format!("function() print({m}) end")),
                hook.continue_execution
            ),
            None => "nil, nil, nil".to_owned(),
        };
        inserts.push((
            *offset,
            format!(
                "if __rbxdbg_B[{line}] or __rbxdbg_D.s then __rbxdbg_H({script_id}, {line}, \"{}\", {{{}}}, {tail}) end ",
                locals.join(" "),
                locals.join(", ")
            ),
        ));
    }
    // Stable sort keeps the prefix ahead of the first statement's hook.
    inserts.sort_by_key(|(offset, _)| *offset);

    let extra: usize = inserts.iter().map(|(_, text)| text.len()).sum();
    let mut out = String::with_capacity(source.len() + extra);
    let mut at = 0;
    for (offset, text) in inserts {
        out.push_str(&source[at..offset]);
        out.push_str(&text);
        at = offset;
    }
    out.push_str(&source[at..]);
    Ok(Instrumented { source: out, lines })
}

fn join(lines: &[u32]) -> String {
    lines
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn declare(scope: &mut Vec<String>, name: &str) {
    if name.starts_with("__rbxdbg") {
        return;
    }
    scope.retain(|n| n != name);
    scope.push(name.to_owned());
}

#[derive(Default)]
struct Walker {
    /// Line → (byte offset of the first statement on it, locals in scope).
    sites: BTreeMap<u32, (usize, Vec<String>)>,
}

/// Collects every function body inside a node.
#[derive(Default)]
struct Bodies(Vec<FunctionBody>);

impl Visitor for Bodies {
    fn visit_function_body(&mut self, node: &FunctionBody) {
        self.0.push(node.clone());
    }
}

fn byte_range(node: &FunctionBody) -> (usize, usize) {
    node.range().map_or((0, 0), |(a, b)| (a.bytes(), b.bytes()))
}

impl Walker {
    fn site<N: Node>(&mut self, node: &N, scope: &[String]) {
        let Some(pos) = node.start_position() else {
            return;
        };
        let line = pos.line() as u32;
        let keep = scope.len().saturating_sub(MAX_LOCALS);
        match self.sites.get(&line) {
            Some((offset, _)) if *offset <= pos.bytes() => {}
            _ => {
                self.sites
                    .insert(line, (pos.bytes(), scope[keep..].to_vec()));
            }
        }
    }

    fn block(&mut self, block: &Block, mut scope: Vec<String>) -> Vec<String> {
        for stmt in block.stmts() {
            self.site(stmt, &scope);
            self.stmt(stmt, &mut scope);
        }
        if let Some(last) = block.last_stmt() {
            self.site(last, &scope);
            self.exprs(last, &scope);
        }
        scope
    }

    /// Walks the outermost anonymous functions inside `node`; each body's
    /// own walk reaches the ones nested deeper.
    fn exprs<N: Visit>(&mut self, node: &N, scope: &[String]) {
        let mut bodies = Bodies::default();
        node.visit(&mut bodies);
        let ranges: Vec<(usize, usize)> = bodies.0.iter().map(byte_range).collect();
        for (i, body) in bodies.0.iter().enumerate() {
            let (s, e) = ranges[i];
            let nested = ranges
                .iter()
                .any(|&(os, oe)| os <= s && e <= oe && (os, oe) != (s, e));
            if !nested {
                self.body(body, scope, false);
            }
        }
    }

    fn body(&mut self, body: &FunctionBody, scope: &[String], method: bool) {
        let mut inner = scope.to_vec();
        if method {
            declare(&mut inner, "self");
        }
        for param in body.parameters() {
            if let Parameter::Name(name) = param {
                declare(&mut inner, &name.token().to_string());
            }
        }
        self.block(body.block(), inner);
    }

    fn stmt(&mut self, stmt: &Stmt, scope: &mut Vec<String>) {
        match stmt {
            Stmt::LocalAssignment(local) => {
                self.exprs(local.expressions(), scope);
                for name in local.names() {
                    declare(scope, &name.token().to_string());
                }
            }
            Stmt::LocalFunction(f) => {
                declare(scope, &f.name().token().to_string());
                self.body(f.body(), scope, false);
            }
            Stmt::FunctionDeclaration(f) => {
                self.body(f.body(), scope, f.name().method_colon().is_some());
            }
            Stmt::NumericFor(f) => {
                self.exprs(f.start(), scope);
                self.exprs(f.end(), scope);
                if let Some(step) = f.step() {
                    self.exprs(step, scope);
                }
                let mut inner = scope.clone();
                declare(&mut inner, &f.index_variable().token().to_string());
                self.block(f.block(), inner);
            }
            Stmt::GenericFor(f) => {
                self.exprs(f.expressions(), scope);
                let mut inner = scope.clone();
                for name in f.names() {
                    declare(&mut inner, &name.token().to_string());
                }
                self.block(f.block(), inner);
            }
            Stmt::If(f) => {
                self.exprs(f.condition(), scope);
                self.block(f.block(), scope.clone());
                for branch in f.else_if().into_iter().flatten() {
                    self.exprs(branch.condition(), scope);
                    self.block(branch.block(), scope.clone());
                }
                if let Some(block) = f.else_block() {
                    self.block(block, scope.clone());
                }
            }
            Stmt::While(w) => {
                self.exprs(w.condition(), scope);
                self.block(w.block(), scope.clone());
            }
            Stmt::Repeat(r) => {
                // `until` sees the loop body's locals.
                let inner = self.block(r.block(), scope.clone());
                self.exprs(r.until(), &inner);
            }
            Stmt::Do(d) => {
                self.block(d.block(), scope.clone());
            }
            Stmt::Assignment(_) | Stmt::FunctionCall(_) | Stmt::CompoundAssignment(_) => {
                self.exprs(stmt, scope);
            }
            _ => {}
        }
    }
}

/// Where a breakpoint on `line` stops: the first hooked line at or after
/// it, if any (`lines` ascending, as `Instrumented::lines`).
pub fn resolve_line(lines: &[u32], line: u32) -> Option<u32> {
    lines.get(lines.partition_point(|&l| l < line)).copied()
}
