//! Touch input for scripts that run in a Play session on a simulated touch
//! device.
//!
//! Sober hands every input to the game as a mouse, so on a simulated phone
//! a game's own touch code (`TouchStarted`, `TouchTap`, `TouchPinch`, an
//! `InputBegan` that checks for `Enum.UserInputType.Touch`) never runs. So
//! a game script is rewritten in place, never across a new line (line
//! numbers, and so error messages and breakpoints, stay as written):
//!
//! - Every way of reaching the input service is wrapped:
//!   `<expr>:GetService("UserInputService")` (and `:FindService`,
//!   `:FindFirstChild`, `:FindFirstChildOfClass`, `:WaitForChild`), a
//!   `GetService` with a computed name, and `<expr>.UserInputService` or
//!   `<expr>["UserInputService"]`:
//!
//!   ```lua
//!   local UIS = require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_touch_input")).wrap(game.UserInputService)
//!   ```
//!
//!   `wrap` checks at run time that it was handed the service, so a
//!   computed name or a table field of the same name passes through.
//! - Every read of an input event (`<expr>.InputBegan`, `.TouchTap`, … see
//!   [`EVENTS`]) goes through `event`, which gives a GuiObject's event the
//!   same touches and gestures the service gets:
//!
//!   ```lua
//!   require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_touch_input")).event(button, "InputBegan"):Connect(f)
//!   ```
//! - A script that says `typeof` gets [`TYPEOF_PREFIX`] before its first
//!   token (after any `--!` directives), so `typeof` reads a stand-in touch
//!   as the `Instance` a real `InputObject` is.
//!
//! Assignment targets (`self.InputBegan = signal`) are left alone. The
//! probe's `__rbxnative_touch_input` module (`probe/touch_input.luau`) is
//! the other half. On a simulated touch device it turns the mouse into a
//! finger (and the wheel into a two-finger pinch or rotate) and reports a
//! touch device; anywhere else, and on the server, every call hands back
//! what it was given.

use full_moon::ast::{
    Assignment, Call, CompoundAssignment, Expression, FunctionArgs, FunctionCall, Index, Prefix,
    Suffix, Var, VarExpression,
};
use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use full_moon::visitors::{Visit, Visitor};
use std::collections::{BTreeMap, HashSet};

/// Name of the ModuleScript the rewritten scripts require.
pub const TOUCH_INPUT_NAME: &str = "__rbxnative_touch_input";
/// Inserted before a service chain; the closing `)` follows the lookup.
pub const WRAP_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_touch_input\")).wrap(";
/// Inserted before an event chain; `.Name` becomes `, "Name")`.
pub const EVENT_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_touch_input\")).event(";
/// Inserted before the first token of a script that says `typeof`.
pub const TYPEOF_PREFIX: &str = "local typeof = require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_touch_input\")).typeof;";

/// Script names whose subtree is never rewritten: a game's fork of the
/// PlayerModule drives the stand-in thumbstick, as the stock one does, and
/// would draw its own touch controls if told the device is a touch device.
pub const SKIPPED_SUBTREES: &[&str] = &["PlayerModule", "ControlScript", "CameraScript"];

/// Events read through `event`: the input events a GuiObject and the
/// service share, and `Activated`, whose first argument is the input.
pub const EVENTS: &[&str] = &[
    "InputBegan",
    "InputChanged",
    "InputEnded",
    "TouchTap",
    "TouchLongPress",
    "TouchPan",
    "TouchSwipe",
    "TouchPinch",
    "TouchRotate",
    "Activated",
];

const SERVICE: &str = "UserInputService";
const LOOKUPS: &[&str] = &[
    "GetService",
    "FindService",
    "FindFirstChild",
    "FindFirstChildOfClass",
    "WaitForChild",
];

/// Rewrites `source` as the module docs describe. `Ok(None)` when there is
/// nothing to rewrite, `Err` with the parser's message when `source` does
/// not parse (the caller publishes it as is).
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    let says_typeof = source.contains("typeof");
    if !says_typeof
        && !source.contains(SERVICE)
        && !source.contains("GetService")
        && !source.contains("FindService")
        && !EVENTS.iter().any(|e| source.contains(e))
    {
        return Ok(None);
    }
    let ast = full_moon::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let mut finder = Finder::default();
    ast.nodes().visit(&mut finder);

    let mut edits = Edits::default();
    let mut seen = HashSet::new();
    for found in finder.found {
        if !seen.insert((found.start, found.at, found.end)) {
            continue;
        }
        if found.last && finder.targets.contains(&(found.start, found.end)) {
            continue;
        }
        match found.kind {
            Kind::Service => {
                edits.open(found.start, found.end, WRAP_OPEN);
                edits.close(found.start, found.end, ")");
            }
            Kind::Event(name) => {
                edits.open(found.start, found.end, EVENT_OPEN);
                let newlines = source[found.at..found.end].matches('\n').count();
                let text = format!(", \"{name}\"){}", "\n".repeat(newlines));
                edits.replace(found.at, found.end, text);
            }
        }
    }
    if says_typeof {
        let first = ast.nodes().tokens().find(|t| {
            !matches!(
                t.token().token_type(),
                TokenType::Whitespace { .. }
                    | TokenType::SingleLineComment { .. }
                    | TokenType::MultiLineComment { .. }
                    | TokenType::Eof
            )
        });
        if let Some(first) = first {
            edits.prefix(first.token().start_position().bytes(), TYPEOF_PREFIX);
        }
    }
    if edits.is_empty() {
        return Ok(None);
    }
    Ok(Some(edits.apply(source)))
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Service,
    Event(&'static str),
}

#[derive(Debug)]
struct Found {
    kind: Kind,
    /// First byte of the chain.
    start: usize,
    /// First byte of the matching suffix.
    at: usize,
    /// Byte after the matching suffix.
    end: usize,
    /// The matching suffix ends the chain.
    last: bool,
}

#[derive(Default)]
struct Finder {
    found: Vec<Found>,
    /// (start, end) of every assignment target.
    targets: HashSet<(usize, usize)>,
}

impl Finder {
    fn chain<'n>(&mut self, prefix: &Prefix, suffixes: impl Iterator<Item = &'n Suffix>) {
        let Some(start) = prefix.start_position().map(|p| p.bytes()) else {
            return;
        };
        let suffixes: Vec<&Suffix> = suffixes.collect();
        for (i, suffix) in suffixes.iter().enumerate() {
            let kind = if is_service(suffix) {
                Kind::Service
            } else if let Some(name) = event_name(suffix) {
                Kind::Event(name)
            } else {
                continue;
            };
            // A bracket index's own end stops at its expression, before `]`.
            let end = match suffix {
                Suffix::Index(Index::Brackets { brackets, .. }) => {
                    brackets.tokens().1.end_position()
                }
                _ => suffix.end_position(),
            };
            let (Some(at), Some(end)) = (suffix.start_position(), end) else {
                continue;
            };
            self.found.push(Found {
                kind,
                start,
                at: at.bytes(),
                end: end.bytes(),
                last: i + 1 == suffixes.len(),
            });
        }
    }

    fn target(&mut self, var: &Var) {
        if let Var::Expression(v) = var {
            if let (Some(s), Some(e)) = (v.start_position(), v.end_position()) {
                self.targets.insert((s.bytes(), e.bytes()));
            }
        }
    }
}

impl Visitor for Finder {
    fn visit_function_call(&mut self, node: &FunctionCall) {
        self.chain(node.prefix(), node.suffixes());
    }

    fn visit_var_expression(&mut self, node: &VarExpression) {
        self.chain(node.prefix(), node.suffixes());
    }

    fn visit_assignment(&mut self, node: &Assignment) {
        for var in node.variables() {
            self.target(var);
        }
    }

    fn visit_compound_assignment(&mut self, node: &CompoundAssignment) {
        self.target(node.lhs());
    }
}

fn is_service(suffix: &Suffix) -> bool {
    match suffix {
        Suffix::Index(Index::Dot { name, .. }) => name.token().to_string() == SERVICE,
        Suffix::Index(Index::Brackets { expression, .. }) => {
            matches!(&**expression, Expression::String(t) if literal(&t.token().to_string()) == Some(SERVICE))
        }
        Suffix::Call(Call::MethodCall(m)) => {
            let name = m.name().token().to_string();
            if !LOOKUPS.contains(&name.as_str()) {
                return false;
            }
            let computed_ok = name == "GetService" || name == "FindService";
            match m.args() {
                FunctionArgs::String(t) => literal(&t.token().to_string()) == Some(SERVICE),
                FunctionArgs::Parentheses { arguments, .. } => {
                    let mut it = arguments.iter();
                    match (it.next(), it.next()) {
                        (Some(Expression::String(t)), None) => {
                            literal(&t.token().to_string()) == Some(SERVICE)
                        }
                        // `GetService(name)`: `wrap` checks the result.
                        (Some(_), None) => computed_ok,
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn event_name(suffix: &Suffix) -> Option<&'static str> {
    let Suffix::Index(Index::Dot { name, .. }) = suffix else {
        return None;
    };
    let name = name.token().to_string();
    EVENTS.iter().copied().find(|e| *e == name)
}

/// The text of a string literal token, `None` for an interpolated or
/// otherwise unexpected one.
fn literal(text: &str) -> Option<&str> {
    match text.strip_prefix("[[") {
        Some(rest) => rest.strip_suffix("]]"),
        None => text
            .strip_prefix(['"', '\''])
            .and_then(|rest| rest.strip_suffix(['"', '\''])),
    }
}

/// Where a text goes among the others inserted at the same position.
type Order = (u8, i64);

/// Insertions and replacements on one source, applied together.
#[derive(Default)]
struct Edits {
    /// Position → (order, text). At one position: the closes of spans
    /// ending there (innermost first), then the opens of spans starting
    /// there (outermost first).
    inserts: BTreeMap<usize, Vec<(Order, String)>>,
    /// Start → (end, text).
    replaces: BTreeMap<usize, (usize, String)>,
}

impl Edits {
    fn open(&mut self, start: usize, end: usize, text: &str) {
        let key = (2, -((end - start) as i64));
        self.inserts
            .entry(start)
            .or_default()
            .push((key, text.to_owned()));
    }

    fn close(&mut self, start: usize, end: usize, text: &str) {
        let key = (1, (end - start) as i64);
        self.inserts
            .entry(end)
            .or_default()
            .push((key, text.to_owned()));
    }

    fn prefix(&mut self, at: usize, text: &str) {
        self.inserts
            .entry(at)
            .or_default()
            .push(((0, 0), text.to_owned()));
    }

    fn replace(&mut self, start: usize, end: usize, text: String) {
        self.replaces.insert(start, (end, text));
    }

    fn is_empty(&self) -> bool {
        self.inserts.is_empty() && self.replaces.is_empty()
    }

    fn apply(mut self, source: &str) -> String {
        let mut points: Vec<usize> = self.inserts.keys().copied().collect();
        points.extend(self.replaces.keys().copied());
        points.sort_unstable();
        points.dedup();
        let mut out = String::with_capacity(source.len() * 2);
        let mut at = 0;
        for point in points {
            if point < at {
                continue;
            }
            out.push_str(&source[at..point]);
            at = point;
            if let Some(mut texts) = self.inserts.remove(&point) {
                texts.sort_by_key(|(key, _)| *key);
                for (_, text) in texts {
                    out.push_str(&text);
                }
            }
            if let Some((end, text)) = self.replaces.remove(&point) {
                out.push_str(&text);
                at = end;
            }
        }
        out.push_str(&source[at..]);
        out
    }
}
