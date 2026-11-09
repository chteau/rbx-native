//! Tree walks that `WeakDom` does not expose directly.

use rbx_dom::{Ref, WeakDom};

/// An instance's parent, from the DOM's own reverse edges: an instance with no
/// parent can still have children, and those are out of reach of a walk from
/// the roots.
pub(crate) fn parent_of(dom: &WeakDom, target: Ref) -> Option<Ref> {
    dom.parent(target)
}

/// `Instance:GetFullName`: names from the topmost ancestor down, joined by
/// periods and left unescaped. The `DataModel` is not a DOM node, so a service
/// is already the top; an unparented instance is a root too.
pub(crate) fn full_name(dom: &WeakDom, target: Ref) -> Option<String> {
    let mut names = Vec::new();
    let mut at = Some(target);
    while let Some(current) = at {
        names.push(dom.get(current)?.name().to_string());
        at = parent_of(dom, current);
    }
    names.reverse();
    Some(names.join("."))
}

pub(crate) fn is_ancestor_of(dom: &WeakDom, ancestor: Ref, target: Ref) -> bool {
    ancestor == target || descendants(dom, ancestor).contains(&target)
}

/// Depth-first pre-order descendants, the order Roblox's `GetDescendants` uses.
pub(crate) fn descendants(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut found = Vec::new();
    let Some(instance) = dom.get(root) else {
        return found;
    };
    for &child in instance.children() {
        found.push(child);
        found.extend(descendants(dom, child));
    }
    found
}

pub(crate) fn find_child(dom: &WeakDom, parent: Ref, name: &str, recursive: bool) -> Option<Ref> {
    let children = dom.get(parent).map(|i| i.children().to_vec())?;
    for child in &children {
        if dom.get(*child).is_some_and(|i| i.name() == name) {
            return Some(*child);
        }
    }
    if !recursive {
        return None;
    }
    children
        .iter()
        .find_map(|child| find_child(dom, *child, name, true))
}

/// The first child (or, with `recursive`, descendant in pre-order) that
/// `test` accepts: the shared walk behind `FindFirstChildOfClass` and
/// `FindFirstChildWhichIsA`.
pub(crate) fn find_child_where(
    dom: &WeakDom,
    parent: Ref,
    recursive: bool,
    test: &dyn Fn(&rbx_dom::Instance) -> bool,
) -> Option<Ref> {
    let children = dom.get(parent).map(|i| i.children().to_vec())?;
    for child in &children {
        if dom.get(*child).is_some_and(test) {
            return Some(*child);
        }
        if recursive {
            if let Some(found) = find_child_where(dom, *child, true, test) {
                return Some(found);
            }
        }
    }
    None
}

/// The nearest ancestor that `test` accepts, for the `FindFirstAncestor*`
/// family. The instance itself is never a candidate.
pub(crate) fn find_ancestor_where(
    dom: &WeakDom,
    target: Ref,
    test: &dyn Fn(&rbx_dom::Instance) -> bool,
) -> Option<Ref> {
    let mut at = parent_of(dom, target);
    while let Some(current) = at {
        if dom.get(current).is_some_and(test) {
            return Some(current);
        }
        at = parent_of(dom, current);
    }
    None
}

/// Copies a subtree under fresh referents, the way `Instance:Clone` does.
pub(crate) fn deep_clone(dom: &mut WeakDom, source: Ref, parent: Option<Ref>) -> Option<Ref> {
    let (class, name, properties, children) = {
        let instance = dom.get(source)?;
        (
            instance.class().to_string(),
            instance.name().to_string(),
            instance.properties().clone(),
            instance.children().to_vec(),
        )
    };

    let copy = dom.new_instance(&class, &name, parent);
    // Bulk-assigning the properties bypasses the change log on purpose: the
    // `Added` record `new_instance` already emitted tells consumers to read the
    // whole instance, so one record per copied property would be noise.
    if let Some(instance) = dom.get_mut(copy) {
        *instance.properties_mut() = properties;
    }
    for child in children {
        deep_clone(dom, child, Some(copy));
    }
    Some(copy)
}
