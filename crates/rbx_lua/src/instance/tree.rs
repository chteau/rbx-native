//! Tree walks that `WeakDom` does not expose directly.

use rbx_dom::{Ref, WeakDom};

/// Finds an instance's parent by walking down from the roots.
///
/// `WeakDom` keeps a reverse edge map but does not expose it, and it has no way to
/// iterate instances either, so the only public route is a walk. Scripts touch
/// `Parent` often but on trees of a few thousand nodes, which stays cheap enough
/// to not justify a second parent index that would have to be kept in sync.
pub(crate) fn parent_of(dom: &WeakDom, target: Ref) -> Option<Ref> {
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        if instance.children().contains(&target) {
            return Some(current);
        }
        stack.extend_from_slice(instance.children());
    }
    None
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
