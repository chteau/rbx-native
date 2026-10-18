//! Wraps a service lookup chain in a game script without moving any line:
//! `<expr>:GetService("X")` becomes `<open><expr>:GetService("X"))`. Used by
//! `market` (purchases) and `touch` (touch input).

use full_moon::ast::{Call, Expression, FunctionArgs, FunctionCall, Prefix, Suffix, VarExpression};
use full_moon::node::Node;
use full_moon::visitors::{Visit, Visitor};

/// Wraps every `<expr>:GetService("<service>")` (and `:FindService`) chain
/// in `source` as `<open><chain>)`. `Ok(None)` when there is nothing to
/// rewrite, `Err` with the parser's message when `source` does not parse.
pub fn wrap_lookups(source: &str, service: &str, open: &str) -> Result<Option<String>, String> {
    if !source.contains(service) {
        return Ok(None);
    }
    let ast = full_moon::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let mut finder = Finder {
        service,
        spans: Vec::new(),
    };
    ast.nodes().visit(&mut finder);
    let mut spans = finder.spans;
    if spans.is_empty() {
        return Ok(None);
    }
    spans.sort_unstable();
    spans.dedup();

    // Closes sort ahead of opens at the same offset, so nested wraps balance.
    let mut edits: Vec<(usize, bool)> = Vec::with_capacity(spans.len() * 2);
    for (start, end) in spans {
        edits.push((start, true));
        edits.push((end, false));
    }
    edits.sort_by_key(|&(at, open)| (at, open));

    let mut out = source.to_owned();
    for (at, is_open) in edits.into_iter().rev() {
        out.insert_str(at, if is_open { open } else { ")" });
    }
    Ok(Some(out))
}

struct Finder<'s> {
    service: &'s str,
    /// (first byte of the chain, byte after the matching suffix).
    spans: Vec<(usize, usize)>,
}

impl Finder<'_> {
    fn chain<'n>(&mut self, prefix: &Prefix, suffixes: impl Iterator<Item = &'n Suffix>) {
        let Some(start) = prefix.start_position().map(|p| p.bytes()) else {
            return;
        };
        for suffix in suffixes {
            if is_lookup(suffix, self.service) {
                if let Some(end) = suffix.end_position() {
                    self.spans.push((start, end.bytes()));
                }
            }
        }
    }
}

fn is_lookup(suffix: &Suffix, service: &str) -> bool {
    let Suffix::Call(Call::MethodCall(m)) = suffix else {
        return false;
    };
    let name = m.name().token().to_string();
    if name != "GetService" && name != "FindService" {
        return false;
    }
    match m.args() {
        FunctionArgs::String(t) => literal_is(&t.token().to_string(), service),
        FunctionArgs::Parentheses { arguments, .. } => {
            let mut it = arguments.iter();
            match (it.next(), it.next()) {
                (Some(Expression::String(t)), None) => literal_is(&t.token().to_string(), service),
                _ => false,
            }
        }
        _ => false,
    }
}

fn literal_is(text: &str, service: &str) -> bool {
    let inner = match text.strip_prefix("[[") {
        Some(rest) => rest.strip_suffix("]]"),
        None => text
            .strip_prefix(['"', '\''])
            .and_then(|rest| rest.strip_suffix(['"', '\''])),
    };
    inner == Some(service)
}

impl Visitor for Finder<'_> {
    fn visit_function_call(&mut self, node: &FunctionCall) {
        self.chain(node.prefix(), node.suffixes());
    }

    fn visit_var_expression(&mut self, node: &VarExpression) {
        self.chain(node.prefix(), node.suffixes());
    }
}
