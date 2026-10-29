//! What a translator would have to translate: the `Text` of every text
//! object `AutoLocalize` leaves on, and every string literal a client script
//! assigns to a `.Text`.

use full_moon::ast::{Assignment, Expression, Var};
use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use full_moon::visitors::Visitor;
use rbx_dom::{Ref, Variant, WeakDom};

/// Text objects and the property each shows: a `TextBox`'s `Text` is the
/// player's own input, so only its placeholder is the game's to translate.
const TEXT_PROPERTIES: [(&str, &str); 3] = [
    ("TextLabel", "Text"),
    ("TextButton", "Text"),
    ("TextBox", "PlaceholderText"),
];

/// Where a string was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    Gui(Ref),
    /// A script, and the 1-based line of the assignment.
    Script(Ref, usize),
}

/// One user-facing string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) text: String,
    /// The holding instance's full name, what a table entry's `Context`
    /// holds: "the full Instance name for the object that the text appeared
    /// on" (Roblox's `LocalizationTable` docs).
    pub(crate) context: String,
    pub(crate) origin: Origin,
}

/// Every user-facing string in `dom`, in tree order, a script's in line
/// order. Text with no letter in it (a number, a symbol) is left out:
/// there is nothing in it to translate.
pub(crate) fn collect(dom: &WeakDom) -> Vec<Found> {
    let mut found = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend(instance.children().iter().rev());
        let property = TEXT_PROPERTIES
            .iter()
            .find(|(class, _)| *class == instance.class())
            .map(|(_, property)| *property);
        if let Some(property) = property.filter(|_| auto_localized(dom, referent)) {
            if let Some(Variant::String(text)) = instance.properties().get(property) {
                push(&mut found, dom, text, Origin::Gui(referent));
            }
        }
        if client_script(dom, referent) {
            if let Some(Variant::String(source)) = instance.properties().get("Source") {
                for (text, line) in assigned_text(source) {
                    push(&mut found, dom, &text, Origin::Script(referent, line));
                }
            }
        }
    }
    found
}

fn push(found: &mut Vec<Found>, dom: &WeakDom, text: &str, origin: Origin) {
    if !text.chars().any(char::is_alphabetic) {
        return;
    }
    let (Origin::Gui(at) | Origin::Script(at, _)) = origin;
    found.push(Found {
        text: text.to_string(),
        context: rbx_audit::full_name(dom, at),
        origin,
    });
}

/// `AutoLocalize` covers "this `GuiBase2d` and its descendants", so one
/// ancestor with it off turns it off.
fn auto_localized(dom: &WeakDom, referent: Ref) -> bool {
    let mut at = Some(referent);
    while let Some(reference) = at {
        let Some(instance) = dom.get(reference) else {
            break;
        };
        if instance.properties().get("AutoLocalize") == Some(&Variant::Bool(false)) {
            return false;
        }
        at = dom.parent(reference);
    }
    true
}

/// A `LocalScript`, or a `Script` whose `RunContext` is Client (2): what
/// runs on the player's machine and so sets text they see.
fn client_script(dom: &WeakDom, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|instance| match instance.class() {
            "LocalScript" => true,
            "Script" => instance.properties().get("RunContext") == Some(&Variant::Enum(2)),
            _ => false,
        })
}

/// Every `<anything>.Text = "literal"` in `source`, with its line. Parsed
/// with `full_moon` the way `rbx_audit` reads scripts, so a string in a
/// comment, or a `.Text` read rather than written, never counts.
pub(crate) fn assigned_text(source: &str) -> Vec<(String, usize)> {
    #[derive(Default)]
    struct Setters(Vec<(String, usize)>);
    impl Visitor for Setters {
        fn visit_assignment(&mut self, node: &Assignment) {
            for (target, value) in node.variables().iter().zip(node.expressions()) {
                if !sets_text(target) {
                    continue;
                }
                let Expression::String(token) = value else {
                    continue;
                };
                let TokenType::StringLiteral { literal, .. } = token.token_type() else {
                    continue;
                };
                let line = value.start_position().map_or(0, |at| at.line());
                self.0.push((unescape(literal), line));
            }
        }
    }
    let ast = full_moon::parse_fallible(source, full_moon::LuaVersion::luau()).into_ast();
    let mut setters = Setters::default();
    setters.visit_ast(&ast);
    setters.0
}

/// Whether `target` ends in `.Text`.
fn sets_text(target: &Var) -> bool {
    let Var::Expression(expression) = target else {
        return false;
    };
    expression
        .suffixes()
        .last()
        .is_some_and(|suffix| suffix.to_string().trim() == ".Text")
}

/// A literal's common escapes resolved; anything rarer is kept as written.
fn unescape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other @ ('"' | '\'' | '\\')) => out.push(other),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
