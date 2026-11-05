//! Playtest recording: the probe's `__rbxnative_record` module samples each
//! player's character (and, from the client, its camera) at a fixed rate
//! and reports gameplay events (`protocol::Uplink::RecSamples`/`RecEvent`).
//!
//! Game code marks its own moments through `shared.rbxnative`, which is nil
//! anywhere but a Play session, so the guarded call costs a shipped game
//! nothing and never errors:
//!
//! ```lua
//! local rbxnative = shared.rbxnative
//! if rbxnative then rbxnative.mark("boss_down", { boss = "Golem" }) end
//! ```
//!
//! A Luau script may read `shared.rbxnative` before the probe has run, so
//! every game script that mentions `rbxnative` gets [`PREFIX`] inserted
//! right before its first token, on that same line (as `crate::print` does).

use std::collections::BTreeMap;

use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use rbx_dom::{Variant, WeakDom};

/// Name of the ModuleScript in ReplicatedStorage.
pub const RECORD_NAME: &str = "__rbxnative_record";

/// Attribute on that module holding the sample rate (a number, samples per
/// second; 0 or absent records nothing).
pub const RATE_ATTRIBUTE: &str = "RATE";

/// Inserted before a script's first token.
pub const PREFIX: &str = "shared.rbxnative = require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_record\")).api;";

/// `source` with [`PREFIX`] inserted before its first token (after any
/// leading whitespace, comments and `--!` directives). `Ok(None)` when the
/// source never names `rbxnative` (as an identifier) or has no token; `Err` with the parser's
/// message when it does not parse.
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    if !source.contains("rbxnative") {
        return Ok(None);
    }
    let ast = full_moon::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    // An identifier, not a substring: the other Play rewrites put
    // `"__rbxnative_…"` strings into the scripts they touch.
    let says_it = ast.nodes().tokens().any(|t| {
        matches!(t.token().token_type(), TokenType::Identifier { identifier } if identifier.as_str() == "rbxnative")
    });
    if !says_it {
        return Ok(None);
    }
    let first = ast.nodes().tokens().find(|t| {
        !matches!(
            t.token().token_type(),
            TokenType::Whitespace { .. }
                | TokenType::SingleLineComment { .. }
                | TokenType::MultiLineComment { .. }
                | TokenType::Eof
        )
    });
    let Some(first) = first else {
        return Ok(None);
    };
    let at = first.token().start_position().bytes();
    let mut out = String::with_capacity(source.len() + PREFIX.len());
    out.push_str(&source[..at]);
    out.push_str(PREFIX);
    out.push_str(&source[at..]);
    Ok(Some(out))
}

/// Sets [`RATE_ATTRIBUTE`] on the injected record module (found by name
/// under ReplicatedStorage, which `crate::inject::inject` created); a no-op
/// when it is missing.
pub fn set_rate(dom: &mut WeakDom, hz: f32) {
    let services = match dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "DataModel"))
    {
        Some(root) => dom
            .get(root)
            .map(|i| i.children().to_vec())
            .unwrap_or_default(),
        None => dom.root_refs().to_vec(),
    };
    let module = services
        .into_iter()
        .filter(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ReplicatedStorage")
        })
        .flat_map(|rs| {
            dom.get(rs)
                .map(|i| i.children().to_vec())
                .unwrap_or_default()
        })
        .find(|c| {
            dom.get(*c)
                .is_some_and(|i| i.class() == "ModuleScript" && i.name() == RECORD_NAME)
        });
    let Some(module) = module else { return };
    let mut attrs = BTreeMap::new();
    attrs.insert(RATE_ATTRIBUTE.to_owned(), Variant::Float64(f64::from(hz)));
    if let Some(raw) = rbx_dom::attributes::encode(&attrs, |_| None) {
        let _ = dom.set_property(
            module,
            "AttributesSerialize",
            Variant::Unknown { type_id: 0x01, raw },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::{inject, ProbeConfig, PROBE_RECORD};

    fn lines(s: &str) -> usize {
        s.matches('\n').count()
    }

    #[test]
    fn prefix_goes_first() {
        let src = "rbxnative.mark('a')\nx()";
        let out = rewrite(src).unwrap().unwrap();
        assert_eq!(out, format!("{PREFIX}{src}"));
        assert_eq!(lines(&out), lines(src));
    }

    #[test]
    fn directive_stays_first() {
        let src = "--!strict\nshared.rbxnative.mark('a')";
        let out = rewrite(src).unwrap().unwrap();
        assert_eq!(
            out,
            format!("--!strict\n{PREFIX}shared.rbxnative.mark('a')")
        );
    }

    #[test]
    fn untouched_without_the_word() {
        assert_eq!(rewrite("local x = 1").unwrap(), None);
    }

    #[test]
    fn another_rewrites_module_name_is_not_the_word() {
        let src = format!("{}print(1)", crate::print::PREFIX);
        assert_eq!(rewrite(&src).unwrap(), None);
    }

    #[test]
    fn parse_error_is_reported() {
        assert!(rewrite("rbxnative(").is_err());
    }

    #[test]
    fn set_rate_sets_and_noops() {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("DataModel", "Game", None);
        set_rate(&mut dom, 5.0);
        let config = ProbeConfig {
            base_url: String::new(),
            token: String::new(),
            session: String::new(),
            ..Default::default()
        };
        let injected = inject(&mut dom, &config).unwrap();
        assert!(dom.get(root).is_some());
        set_rate(&mut dom, 5.0);
        let attr = dom
            .get(injected.record)
            .unwrap()
            .attributes()
            .remove(RATE_ATTRIBUTE);
        assert_eq!(attr, Some(Variant::Float64(5.0)));
    }

    #[test]
    fn probe_source_has_server_entry() {
        assert!(!PROBE_RECORD.is_empty());
        assert!(PROBE_RECORD.contains("startServer"));
    }
}
