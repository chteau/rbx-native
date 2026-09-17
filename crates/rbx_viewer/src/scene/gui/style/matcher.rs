//! Whether one parsed selector picks out one instance.

use rbx_dom::{Ref, WeakDom};

use super::selector::{Combinator, Compound, Selector};

/// Whether `referent` is one of the instances `selector` styles.
///
/// The walk stops at `root`, the `StyleLink`'s parent: the sheet applies to
/// that tree (`StyleLink`, creator-docs), so an ancestor step never escapes
/// it. The docs do not say what an ancestor selector above the root should do;
/// not matching keeps a sheet's reach the tree it was linked to.
pub(super) fn matches(dom: &WeakDom, root: Ref, referent: Ref, selector: &Selector) -> bool {
    if selector.inactive || !compound_matches(dom, referent, &selector.subject) {
        return false;
    }
    step(dom, root, referent, &selector.ancestors)
}

/// The ancestor steps, nearest first, each starting from the instance the
/// previous one matched.
fn step(dom: &WeakDom, root: Ref, from: Ref, ancestors: &[(Combinator, Compound)]) -> bool {
    let Some(((combinator, compound), rest)) = ancestors.split_first() else {
        return true;
    };
    if from == root {
        return false;
    }
    let mut candidate = dom.parent(from);
    while let Some(referent) = candidate {
        if compound_matches(dom, referent, compound) && step(dom, root, referent, rest) {
            return true;
        }
        // A child combinator gets the one look; a descendant one keeps
        // climbing, which is what makes `A >> B >> C` need the backtracking
        // this recursion provides.
        if *combinator == Combinator::Child || referent == root {
            return false;
        }
        candidate = dom.parent(referent);
    }
    false
}

fn compound_matches(dom: &WeakDom, referent: Ref, compound: &Compound) -> bool {
    let Some(instance) = dom.get(referent) else {
        return false;
    };
    if compound
        .class
        .as_deref()
        .is_some_and(|class| class != instance.class())
    {
        return false;
    }
    if compound
        .name
        .as_deref()
        .is_some_and(|name| name != instance.name())
    {
        return false;
    }
    if compound.tags.is_empty() {
        return true;
    }
    let tags = instance.tags();
    compound
        .tags
        .iter()
        .all(|wanted| tags.contains(&wanted.as_str()))
}
