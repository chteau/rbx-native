//! Purchase mocking for scripts that run in a Play session.
//!
//! A Play session publishes the place to a private sandbox, where a real
//! `MarketplaceService` prompt would charge real Robux. So every
//! `<expr>:GetService("MarketplaceService")` (and `:FindService`) in a game
//! script is wrapped in place, never across a new line (line numbers, and so
//! error messages and breakpoints, stay as the developer wrote them):
//!
//! ```lua
//! require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_market")).wrap(game:GetService("MarketplaceService")):PromptProductPurchase(p, 1)
//! ```
//!
//! Only the lookup is wrapped; any later suffix of the chain stays outside,
//! so it runs on the proxy. The probe's `__rbxnative_market` module
//! (`probe/market.luau`) is the other half: its proxy shows our own prompt
//! and drives `ProcessReceipt` locally.

use full_moon::ast::{Call, Expression, FunctionArgs, FunctionCall, Prefix, Suffix, VarExpression};
use full_moon::node::Node;
use full_moon::visitors::{Visit, Visitor};

/// Name of the ModuleScript the rewritten scripts require.
pub const MARKET_NAME: &str = "__rbxnative_market";
/// Name of the ModuleScript holding the purchase dialog.
pub const MARKET_UI_NAME: &str = "__rbxnative_market_ui";
/// Inserted before a matching chain; the closing `)` follows the lookup.
pub const WRAP_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_market\")).wrap(";

const SERVICE: &str = "MarketplaceService";

/// Wraps every `MarketplaceService` lookup in `source`. `Ok(None)` when there
/// is nothing to rewrite, `Err` with the parser's message when `source` does
/// not parse (the caller publishes it as is).
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    if !source.contains(SERVICE) {
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
    for (at, open) in edits.into_iter().rev() {
        out.insert_str(at, if open { WRAP_OPEN } else { ")" });
    }
    Ok(Some(out))
}

#[derive(Default)]
struct Finder {
    /// (first byte of the chain, byte after the matching suffix).
    spans: Vec<(usize, usize)>,
}

impl Finder {
    fn chain<'n>(&mut self, prefix: &Prefix, suffixes: impl Iterator<Item = &'n Suffix>) {
        let Some(start) = prefix.start_position().map(|p| p.bytes()) else {
            return;
        };
        for suffix in suffixes {
            if is_lookup(suffix) {
                if let Some(end) = suffix.end_position() {
                    self.spans.push((start, end.bytes()));
                }
            }
        }
    }
}

fn is_lookup(suffix: &Suffix) -> bool {
    let Suffix::Call(Call::MethodCall(m)) = suffix else {
        return false;
    };
    let name = m.name().token().to_string();
    if name != "GetService" && name != "FindService" {
        return false;
    }
    match m.args() {
        FunctionArgs::String(t) => literal_is_service(&t.token().to_string()),
        FunctionArgs::Parentheses { arguments, .. } => {
            let mut it = arguments.iter();
            match (it.next(), it.next()) {
                (Some(Expression::String(t)), None) => literal_is_service(&t.token().to_string()),
                _ => false,
            }
        }
        _ => false,
    }
}

fn literal_is_service(text: &str) -> bool {
    let inner = match text.strip_prefix("[[") {
        Some(rest) => rest.strip_suffix("]]"),
        None => text
            .strip_prefix(['"', '\''])
            .and_then(|rest| rest.strip_suffix(['"', '\''])),
    };
    inner == Some(SERVICE)
}

impl Visitor for Finder {
    fn visit_function_call(&mut self, node: &FunctionCall) {
        self.chain(node.prefix(), node.suffixes());
    }

    fn visit_var_expression(&mut self, node: &VarExpression) {
        self.chain(node.prefix(), node.suffixes());
    }
}
