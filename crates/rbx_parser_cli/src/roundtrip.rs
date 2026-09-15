//! `--roundtrip` diagnostic: re-serializes a loaded DOM through both `rbx_binary`
//! and `rbx_xml`, deserializes the result again, and checks the new tree matches
//! the original one (class, name, parent, every property including `Unknown`
//! blobs).
//!
//! Referents are not guaranteed to survive a round-trip identically (the XML
//! writer/reader in particular assigns fresh ones), so both trees are walked in
//! the same deterministic pre-order — root list, then each instance's
//! `children()` in order, which every serializer/deserializer in this
//! workspace preserves — and instances are paired up by position in that walk.
//! This mirrors `rbx_xml/tests/serialize_round_trip.rs`'s approach; it is
//! reimplemented here rather than shared because that test module is
//! `#[cfg(test)]`-only and not reachable from this binary crate.

use std::collections::HashMap;

use rbx_dom::{Content, Ref, Variant, WeakDom};

/// Outcome of one format's round-trip check.
pub struct CheckResult {
    pub label: &'static str,
    pub outcome: Result<(), String>,
}

/// Runs both round-trip checks against an already-loaded DOM.
pub fn run(dom: &WeakDom) -> Vec<CheckResult> {
    vec![check_binary(dom), check_xml(dom)]
}

/// Prints an OK/FAIL line per check, with the failure detail indented below a
/// FAIL. Returns whether every check passed.
pub fn report(results: &[CheckResult]) -> bool {
    let mut all_ok = true;
    for result in results {
        match &result.outcome {
            Ok(()) => println!("{}: OK", result.label),
            Err(message) => {
                all_ok = false;
                println!("{}: FAIL", result.label);
                println!("  {message}");
            }
        }
    }
    all_ok
}

fn check_binary(dom: &WeakDom) -> CheckResult {
    let label = "binary round-trip (rbx_binary)";
    let bytes = match rbx_binary::serialize(dom) {
        Ok(bytes) => bytes,
        Err(err) => return fail(label, format!("serialize failed: {err}")),
    };
    let after = match rbx_binary::deserialize(&bytes) {
        Ok(dom) => dom,
        Err(err) => return fail(label, format!("re-deserialize failed: {err}")),
    };
    match compare_trees(dom, &after) {
        Ok(()) => ok(label),
        Err(message) => fail(label, message),
    }
}

fn check_xml(dom: &WeakDom) -> CheckResult {
    let label = "xml round-trip (rbx_xml)";
    let xml = match rbx_xml::serialize(dom) {
        Ok(xml) => xml,
        Err(err) => return fail(label, format!("serialize failed: {err}")),
    };
    let after = match rbx_xml::deserialize(&xml) {
        Ok(dom) => dom,
        Err(err) => return fail(label, format!("re-deserialize failed: {err}")),
    };
    match compare_trees(dom, &after) {
        Ok(()) => ok(label),
        Err(message) => fail(label, message),
    }
}

fn ok(label: &'static str) -> CheckResult {
    CheckResult {
        label,
        outcome: Ok(()),
    }
}

fn fail(label: &'static str, message: String) -> CheckResult {
    CheckResult {
        label,
        outcome: Err(message),
    }
}

// Pre-order walk plus a parent-of-referent map built in the same pass, so
// looking up an instance's parent (for structural comparison and for
// rendering a root-to-instance path in a report) doesn't need a second,
// quadratic scan over the tree.
fn preorder_with_parents(dom: &WeakDom) -> (Vec<Ref>, HashMap<Ref, Option<Ref>>) {
    fn visit(
        dom: &WeakDom,
        referent: Ref,
        parent: Option<Ref>,
        order: &mut Vec<Ref>,
        parents: &mut HashMap<Ref, Option<Ref>>,
    ) {
        order.push(referent);
        parents.insert(referent, parent);
        if let Some(instance) = dom.get(referent) {
            for &child in instance.children() {
                visit(dom, child, Some(referent), order, parents);
            }
        }
    }

    let mut order = Vec::new();
    let mut parents = HashMap::new();
    for &root in dom.root_refs() {
        visit(dom, root, None, &mut order, &mut parents);
    }
    (order, parents)
}

// Root-to-`referent` instance names, joined with `/`, for the failing instance
// in a report; cheap because it only runs once, on the first mismatch found.
fn path_to(dom: &WeakDom, parents: &HashMap<Ref, Option<Ref>>, referent: Ref) -> String {
    let mut segments = Vec::new();
    let mut current = referent;
    loop {
        let name = dom.get(current).map(|i| i.name()).unwrap_or("<unknown>");
        segments.push(name.to_string());
        match parents.get(&current).copied().flatten() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    segments.reverse();
    segments.join("/")
}

// Rewrites the `Ref`s embedded inside a property value through `map`, so a
// value from the original tree can be compared against its round-tripped
// counterpart even when the two trees disagree on raw `Ref` numbering. A
// referent absent from `map` is left as-is: `Variant::Ref`/`Content::Object`
// never encode Roblox's null referent (that decodes as a missing property or
// an empty `Content` instead), so this only fires on a genuinely broken
// round-trip, and falling through here lets the ensuing comparison report it
// as a property mismatch instead of panicking.
fn remap(value: &Variant, map: &HashMap<Ref, Ref>) -> Variant {
    match value {
        Variant::Ref(r) => Variant::Ref(map.get(r).copied().unwrap_or(*r)),
        Variant::Content(Content::Object(r)) => {
            Variant::Content(Content::Object(map.get(r).copied().unwrap_or(*r)))
        }
        other => other.clone(),
    }
}

// Merge-join over two sorted (`BTreeMap`) property lists: returns the first
// key where the two sides disagree, whichever side it's missing from.
fn first_property_mismatch<'a>(
    expected: &'a std::collections::BTreeMap<String, Variant>,
    actual: &'a std::collections::BTreeMap<String, Variant>,
) -> Option<(&'a str, Option<&'a Variant>, Option<&'a Variant>)> {
    let mut a = expected.iter().peekable();
    let mut b = actual.iter().peekable();
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => return None,
            (Some(&(name, value)), None) => {
                a.next();
                return Some((name, Some(value), None));
            }
            (None, Some(&(name, value))) => {
                b.next();
                return Some((name, None, Some(value)));
            }
            (Some(&(a_name, a_value)), Some(&(b_name, b_value))) => match a_name.cmp(b_name) {
                std::cmp::Ordering::Equal => {
                    if a_value != b_value {
                        return Some((a_name, Some(a_value), Some(b_value)));
                    }
                    a.next();
                    b.next();
                }
                std::cmp::Ordering::Less => {
                    a.next();
                    return Some((a_name, Some(a_value), None));
                }
                std::cmp::Ordering::Greater => {
                    b.next();
                    return Some((b_name, None, Some(b_value)));
                }
            },
        }
    }
}

const MAX_VALUE_CHARS: usize = 200;

// Caps a property value's `Debug` rendering so a giant `Unknown` blob (or a
// long sequence/keypoint list) doesn't turn one mismatch into a wall of text.
fn fmt_value(value: Option<&Variant>) -> String {
    let text = match value {
        Some(v) => format!("{v:?}"),
        None => return "<absent>".to_string(),
    };
    if text.chars().count() > MAX_VALUE_CHARS {
        let truncated: String = text.chars().take(MAX_VALUE_CHARS).collect();
        format!("{truncated}... ({} chars total)", text.chars().count())
    } else {
        text
    }
}

fn compare_trees(before: &WeakDom, after: &WeakDom) -> Result<(), String> {
    let (before_order, before_parents) = preorder_with_parents(before);
    let (after_order, after_parents) = preorder_with_parents(after);

    if before_order.len() != after_order.len() {
        return Err(format!(
            "instance count changed: {} before, {} after",
            before_order.len(),
            after_order.len()
        ));
    }

    let ref_map: HashMap<Ref, Ref> = before_order
        .iter()
        .copied()
        .zip(after_order.iter().copied())
        .collect();

    for (&original_ref, &round_tripped_ref) in before_order.iter().zip(after_order.iter()) {
        // Both refs come from a walk of their own DOM, so `get` cannot fail here.
        let original = before.get(original_ref).expect("walked from before");
        let round_tripped = after.get(round_tripped_ref).expect("walked from after");
        let path = path_to(before, &before_parents, original_ref);

        if original.class() != round_tripped.class() {
            return Err(format!(
                "{path}: class changed: expected {:?}, actual {:?}",
                original.class(),
                round_tripped.class()
            ));
        }
        if original.name() != round_tripped.name() {
            return Err(format!(
                "{path} ({}): name changed: expected {:?}, actual {:?}",
                original.class(),
                original.name(),
                round_tripped.name()
            ));
        }

        let expected_parent = before_parents[&original_ref].map(|p| ref_map[&p]);
        let actual_parent = after_parents[&round_tripped_ref];
        if expected_parent != actual_parent {
            return Err(format!(
                "{path} ({} {:?}): parent changed",
                original.class(),
                original.name()
            ));
        }

        let expected_properties: std::collections::BTreeMap<String, Variant> = original
            .properties()
            .iter()
            .map(|(name, value)| (name.clone(), remap(value, &ref_map)))
            .collect();
        if let Some((name, expected, actual)) =
            first_property_mismatch(&expected_properties, round_tripped.properties())
        {
            return Err(format!(
                "{path} ({} {:?}): property `{name}` changed: expected {}, actual {}",
                original.class(),
                original.name(),
                fmt_value(expected),
                fmt_value(actual)
            ));
        }
    }

    Ok(())
}
