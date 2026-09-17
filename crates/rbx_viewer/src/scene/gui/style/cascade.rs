//! Flattening a `StyleSheet` — its derives, its nested rules and its tokens —
//! into the flat, ordered list of rules the cascade applies.

use std::collections::{BTreeMap, HashSet};

use rbx_dom::{Instance, Ref, Variant, WeakDom};

use super::selector::{self, Selector};

const RULE_CLASS: &str = "StyleRule";
const DERIVE_CLASS: &str = "StyleDerive";

/// How deep a `$Token` may point at another token before this gives up. A
/// token sheet, a theme sheet and the design sheet are three; the cap only
/// exists so a sheet that references itself terminates.
const TOKEN_DEPTH: usize = 8;

/// One `StyleRule`, its `$Token` references already resolved.
pub(super) struct Rule {
    pub(super) selectors: Vec<Selector>,
    pub(super) properties: BTreeMap<String, Variant>,
    priority: i32,
    /// Position in the flattened sheet, which is what breaks a priority tie.
    order: usize,
}

/// Every rule `sheet` brings to bear, weakest first — apply them in this order
/// and the strongest is the one left standing.
///
/// Precedence is `StyleRule.Priority` (higher wins, per the docs), and for a
/// tie the later rule in this flattening. The docs describe no CSS-style
/// specificity — a `#Name` selector carries no more weight than a class one —
/// and say nothing about ties, so "later wins" is borrowed from CSS. Derived
/// sheets are flattened first because `StyleSheet:SetDerives` calls the base
/// sheet "the spot of least priority".
pub(super) fn flatten(dom: &WeakDom, sheet: Ref) -> Vec<Rule> {
    let mut rules = Vec::new();
    collect(
        dom,
        sheet,
        &mut HashSet::new(),
        &mut BTreeMap::new(),
        &mut rules,
    );
    for (order, rule) in rules.iter_mut().enumerate() {
        rule.order = order;
    }
    rules.sort_by_key(|rule| (rule.priority, rule.order));
    rules
}

/// Depth-first through the derive graph, accumulating both tokens and rules in
/// weakest-first order. `seen` stops a sheet that derives from itself.
fn collect(
    dom: &WeakDom,
    sheet: Ref,
    seen: &mut HashSet<Ref>,
    tokens: &mut BTreeMap<String, Variant>,
    rules: &mut Vec<Rule>,
) {
    let Some(instance) = dom.get(sheet) else {
        return;
    };
    if !seen.insert(sheet) {
        return;
    }

    // `StyleDerive.Priority`: higher takes precedence, so the lowest is laid
    // down first and everything after it writes over the top.
    let mut derives: Vec<(i32, Ref)> = instance
        .children()
        .iter()
        .filter_map(|&child| {
            let derive = dom.get(child)?;
            (derive.class() == DERIVE_CLASS)
                .then(|| Some((integer(derive, "Priority"), reference(derive)?)))
                .flatten()
        })
        .collect();
    derives.sort_by_key(|&(priority, _)| priority);
    for (_, derived) in derives {
        collect(dom, derived, seen, tokens, rules);
    }

    // The sheet's own tokens beat anything it derived them over.
    tokens.extend(instance.attributes());
    for &child in instance.children() {
        read_rule(dom, child, "", tokens, rules);
    }
}

/// One `StyleRule` and, beneath it, the rules nested inside it.
///
/// A nested rule's selector merges with its parent's (css-comparisons,
/// "Nesting and merging"): `#MenuFrame` with a nested `> TextButton` styles
/// `#MenuFrame > TextButton`, and a further nested `:Hover` appends to that
/// same compound. `outer_tokens` are the enclosing rules' attributes, which a
/// nested rule inherits along with the selector.
fn read_rule(
    dom: &WeakDom,
    referent: Ref,
    outer_selector: &str,
    outer_tokens: &BTreeMap<String, Variant>,
    rules: &mut Vec<Rule>,
) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    if instance.class() != RULE_CLASS {
        return;
    }

    let selector = merge(outer_selector, text(instance, "Selector"));
    let mut tokens = outer_tokens.clone();
    tokens.extend(instance.attributes());

    let properties = rbx_dom::attributes::decode(instance.properties().get("PropertiesSerialize"))
        .into_iter()
        .filter_map(|(name, value)| Some((name, resolve(&tokens, &value, 0)?)))
        .collect();
    rules.push(Rule {
        selectors: selector::parse(&selector),
        properties,
        priority: integer(instance, "Priority"),
        order: 0,
    });

    for &child in instance.children() {
        read_rule(dom, child, &selector, &tokens, rules);
    }
}

/// A nested rule's selector written out in full.
///
/// The docs only show a nested selector that opens with a combinator (`>`) or
/// with a compound part (`:`, `.`, `#`, `::`). A bare class name is left
/// undescribed; it is read as a descendant, the reading that matches SCSS.
fn merge(outer: &str, inner: &str) -> String {
    let inner = inner.trim();
    if outer.is_empty() {
        return inner.to_owned();
    }
    match inner.chars().next() {
        None => outer.to_owned(),
        Some(':' | '.' | '#' | '@') => format!("{outer}{inner}"),
        Some('>') => format!("{outer} {inner}"),
        Some(_) => format!("{outer} >> {inner}"),
    }
}

/// A property value with any `$Token` reference followed to what it stands for.
///
/// Tokens are `StyleSheet`/`StyleRule` attributes named with a `$` prefix
/// (`StyleRule:SetProperty`, creator-docs). A value that names no token is
/// itself; an unresolvable one drops the property, since a literal `"$Name"`
/// string is indistinguishable from a broken reference in a saved file.
fn resolve(tokens: &BTreeMap<String, Variant>, value: &Variant, depth: usize) -> Option<Variant> {
    let Variant::String(text) = value else {
        return Some(value.clone());
    };
    let Some(name) = text.strip_prefix('$') else {
        return Some(value.clone());
    };
    if depth >= TOKEN_DEPTH {
        return None;
    }
    resolve(tokens, tokens.get(name)?, depth + 1)
}

fn integer(instance: &Instance, name: &str) -> i32 {
    match instance.properties().get(name) {
        Some(&Variant::Int32(value)) => value,
        _ => 0,
    }
}

fn text<'a>(instance: &'a Instance, name: &str) -> &'a str {
    match instance.properties().get(name) {
        Some(Variant::String(value)) => value,
        _ => "",
    }
}

/// The `StyleSheet` reference a `StyleLink` or `StyleDerive` carries. Whether
/// it still points at anything is [`WeakDom::get`]'s answer, not this one's.
pub(super) fn reference(instance: &Instance) -> Option<Ref> {
    match instance.properties().get("StyleSheet") {
        Some(&Variant::Ref(referent)) => Some(referent),
        _ => None,
    }
}
