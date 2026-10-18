//! Lays a merge's output over the live DOM it was taken from.

use std::collections::{HashMap, HashSet};

use crate::attributes::remap_refs;
use crate::{Instance, Ref, Variant, WeakDom};

/// Makes `target` change the way `to` differs from `from`, through
/// `WeakDom`'s tracked mutations only (so the change log reports every
/// edit): `from` and `to` share referents (`to` is a `Merge::output` and
/// `from` the `Merge::ours` it started from); `target` is the DOM `from`
/// was copied from, possibly edited since.
///
/// - An instance in `from` but not `to` is removed from `target`.
/// - An instance in `to` but not `from` is inserted (subtree roots first,
///   then their descendants); if its referent is already taken in
///   `target` it gets `target.new_instance`'s next one instead, and every
///   `Ref` property and `Instance` attribute (`attributes::remap_refs`)
///   written by this call is translated accordingly.
/// - For an instance in both: name, class, each property set or removed
///   where `to` differs from `from`; the parent set when it differs; then
///   children ordered as `to` orders them (`place_after`).
/// - An instance `target` no longer has is skipped, with its edits.
pub fn apply(target: &mut WeakDom, from: &WeakDom, to: &WeakDom) {
    let from_order = preorder(from);
    let to_order = preorder(to);

    for &r in &from_order {
        if to.get(r).is_none() && target.get(r).is_some() {
            target.remove(r);
        }
    }

    let added: Vec<Ref> = to_order
        .iter()
        .copied()
        .filter(|r| from.get(*r).is_none())
        .collect();
    let map = allocate(target, &added);
    let translate = |v: &Variant| -> Variant {
        if let Variant::Ref(r) = v {
            return Variant::Ref(map.get(r).copied().unwrap_or(*r));
        }
        remap_refs(v, |r| Some(map.get(&r).copied().unwrap_or(r))).unwrap_or_else(|| v.clone())
    };

    let mut inserted: HashSet<Ref> = HashSet::new();
    for &r in &added {
        let Some(src) = to.get(r) else { continue };
        let parent = to.parent(r).map(|p| map.get(&p).copied().unwrap_or(p));
        if let Some(p) = parent {
            if target.get(p).is_none() {
                continue;
            }
        }
        let new_ref = map[&r];
        let mut instance = Instance::new(new_ref, src.class(), src.name());
        for (key, value) in src.properties() {
            instance
                .properties_mut()
                .insert(key.clone(), translate(value));
        }
        target.insert(instance);
        if parent.is_some() {
            target.set_parent(new_ref, parent);
        }
        inserted.insert(r);
    }

    let live = |r: Ref| -> Option<Ref> {
        if from.get(r).is_some() {
            target.get(r).map(|_| r)
        } else if inserted.contains(&r) {
            Some(map[&r])
        } else {
            None
        }
    };
    let resolve: HashMap<Ref, Ref> = to_order
        .iter()
        .filter_map(|&r| live(r).map(|t| (r, t)))
        .collect();

    for &r in &to_order {
        let (Some(old), Some(new)) = (from.get(r), to.get(r)) else {
            continue;
        };
        if target.get(r).is_none() {
            continue;
        }
        if old.name() != new.name() {
            let _ = target.set_name(r, new.name());
        }
        if old.class() != new.class() {
            let _ = target.set_class(r, new.class());
        }
        for (key, value) in new.properties() {
            if old.properties().get(key) != Some(value) {
                let _ = target.set_property(r, key, translate(value));
            }
        }
        for key in old.properties().keys() {
            if !new.properties().contains_key(key) {
                let _ = target.remove_property(r, key);
            }
        }
        let (old_parent, new_parent) = (from.parent(r), to.parent(r));
        if old_parent != new_parent {
            match new_parent {
                None => target.set_parent(r, None),
                Some(p) => {
                    if let Some(&tp) = resolve.get(&p) {
                        target.set_parent(r, Some(tp));
                    }
                }
            }
        }
    }

    for &p in &to_order {
        let Some(&tp) = resolve.get(&p) else { continue };
        let mut prev: Option<Ref> = None;
        for &c in to.get(p).map(|i| i.children()).unwrap_or_default() {
            let Some(&tc) = resolve.get(&c) else { continue };
            if target.parent(tc) != Some(tp) {
                continue;
            }
            if let Some(anchor) = prev {
                target.place_after(tc, anchor);
            }
            prev = Some(tc);
        }
    }
}

fn preorder(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        out.push(r);
        if let Some(i) = dom.get(r) {
            stack.extend(i.children().iter().rev().copied());
        }
    }
    out
}

/// Gives each added instance its output referent when `target` has it free,
/// else a fresh one clear of everything `target` or this call uses.
fn allocate(target: &WeakDom, added: &[Ref]) -> HashMap<Ref, Ref> {
    let mut map = HashMap::new();
    let mut used: HashSet<Ref> = added.iter().copied().collect();
    let mut next = target.next_ref();
    for &r in added {
        if target.get(r).is_none() {
            map.insert(r, r);
        }
    }
    for &r in added {
        if map.contains_key(&r) {
            continue;
        }
        let fresh = loop {
            let candidate = Ref::new(next);
            next += 1;
            if target.get(candidate).is_none() && !used.contains(&candidate) {
                break candidate;
            }
        };
        used.insert(fresh);
        map.insert(r, fresh);
    }
    map
}
