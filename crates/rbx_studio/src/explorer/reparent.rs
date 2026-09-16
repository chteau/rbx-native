//! Which Explorer drag-and-drop moves the tree will accept.
//!
//! Studio reparents by dropping *onto* a row, never between two of them:
//! creator-docs' own Explorer page puts it as "to change the parent of one or
//! more children (reparent), simply drag and drop them onto the new parent".
//! There is no before/after insertion position to aim at because a place has
//! no user-orderable sibling order in the first place — the Explorer sorts
//! what it shows (see this module's parent). So a drop target is one
//! instance, and everything here is about which `(dragged set, target)` pairs
//! are legal.
//!
//! [`WeakDom::set_parent`] validates nothing: parenting an instance under its
//! own descendant detaches that whole subtree from every root while leaving
//! the cycle intact, and nothing would ever draw it again. The rules below are
//! the only thing standing between a careless drop and that, so they run
//! before the move, not after.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// The instances a drop onto `target` would actually move, in the order they
/// were dragged. Empty means the drop is refused — nothing to move is the same
/// answer as nothing allowed to.
///
/// Returning the set rather than a bare `bool` keeps one rule behind the
/// highlight under the cursor, the drop itself and these tests, instead of
/// three that can drift apart.
pub(crate) fn movable(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    dragged: &[Ref],
    target: Ref,
) -> Vec<Ref> {
    if dom.get(target).is_none() {
        return Vec::new();
    }

    dragged
        .iter()
        .copied()
        .filter(|&reference| moves(dom, database, dragged, reference, target))
        .collect()
}

/// Whether dropping `dragged` onto `target` would move anything at all — the
/// predicate the hovered row's highlight and GPUI's `can_drop` ask.
pub(crate) fn accepts(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    dragged: &[Ref],
    target: Ref,
) -> bool {
    !movable(dom, database, dragged, target).is_empty()
}

/// Whether this one instance out of `dragged` ends up somewhere new.
fn moves(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    dragged: &[Ref],
    reference: Ref,
    target: Ref,
) -> bool {
    let Some(instance) = dom.get(reference) else {
        return false;
    };
    // Roblox creates exactly one of each service and parents it to the
    // DataModel; Studio's Explorer will not let you drag one somewhere else,
    // and a place whose Workspace lived inside Lighting is not a place the
    // rest of this editor (or the file format) knows how to express. Note
    // this is narrower than "is a root": a `.rbxm` opens with ordinary
    // instances at the root, and those are perfectly movable.
    if database.is_service(instance.class()) {
        return false;
    }
    // Onto itself, or into its own subtree: the cycle this whole module
    // exists to refuse.
    if reference == target || is_ancestor(dom, reference, target) {
        return false;
    }
    // Already there. Refusing rather than re-parenting in place is what keeps
    // a drop onto the row's own parent from pushing an undo step that undoes
    // nothing.
    if dom.parent(reference) == Some(target) {
        return false;
    }
    // Dropped together with something it already sits inside: moving the
    // ancestor carries this one along, and pulling it out of that ancestor on
    // the way is a second rearrangement nobody asked for.
    !dragged
        .iter()
        .any(|&other| other != reference && is_ancestor(dom, other, reference))
}

/// Whether `ancestor` is somewhere above `reference`, walking the parent chain
/// rather than the subtree below: this runs for every visible row on every
/// frame of a drag, and a service's subtree is the whole place.
///
/// The visited list bounds the walk. A tree this editor built cannot contain a
/// cycle — these rules are what stop one — but a malformed file is not this
/// function's problem to diagnose, and hanging over one is worse than
/// answering "no".
fn is_ancestor(dom: &WeakDom, ancestor: Ref, reference: Ref) -> bool {
    let mut seen = Vec::new();
    let mut current = dom.parent(reference);
    while let Some(parent) = current {
        if parent == ancestor {
            return true;
        }
        if seen.contains(&parent) {
            return false;
        }
        seen.push(parent);
        current = dom.parent(parent);
    }
    false
}

#[cfg(test)]
#[path = "reparent/tests.rs"]
mod tests;
