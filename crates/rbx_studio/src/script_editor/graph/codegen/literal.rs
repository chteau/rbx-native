//! Literals typed into a pin, checked against the pin's type and written
//! as Luau, and the names the generated code may give its locals.

use super::super::catalog::{PinType, Prec};

/// Whether an expression can be indexed or called as written: a name or a
/// call can, a literal string or number cannot.
pub(crate) fn indexable(text: &str) -> bool {
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

/// Names a fresh local must not take: Luau's keywords and the globals a
/// generated script reads.
pub(super) const TAKEN: &[&str] = &[
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

pub(super) fn lower_first(text: &str) -> String {
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
