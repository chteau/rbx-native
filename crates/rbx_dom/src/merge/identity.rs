//! Pairs the instances of two DOMs that are the same instance.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{Ref, UniqueId, Variant, WeakDom};

/// `a`'s referents mapped to the `b` instance each is, one-to-one.
///
/// First by `UniqueId` (a `Variant::UniqueId` under the `UniqueId`
/// property), counting only ids that occur exactly once in each DOM. Then,
/// when `by_position` is set, top-down from the roots and from every pair
/// already made: under a matched parent pair (or among the roots), the
/// i-th still-unmatched child of a given (class, name) in `a` pairs with the
/// i-th still-unmatched one in `b`, skipping candidates where both carry a
/// `UniqueId` and the ids differ. Pairs made this way are descended into
/// in turn, so a subtree without ids matches whole under a matched parent.
pub(crate) fn match_doms(a: &WeakDom, b: &WeakDom, by_position: bool) -> HashMap<Ref, Ref> {
    let ids_a = unique_ids(a);
    let ids_b = unique_ids(b);
    let mut pairs: HashMap<Ref, Ref> = HashMap::new();
    for (id, &ra) in &ids_a {
        if let Some(&rb) = ids_b.get(id) {
            pairs.insert(ra, rb);
        }
    }
    if !by_position {
        return pairs;
    }

    let mut used: HashSet<Ref> = pairs.values().copied().collect();
    let mut queue: VecDeque<(Option<Ref>, Option<Ref>)> = VecDeque::new();
    queue.push_back((None, None));
    queue.extend(pairs.iter().map(|(&x, &y)| (Some(x), Some(y))));

    while let Some((pa, pb)) = queue.pop_front() {
        let kids_a = children(a, pa);
        let kids_b = children(b, pb);
        let mut candidates: HashMap<(&str, &str), Vec<Ref>> = HashMap::new();
        for &rb in &kids_b {
            if used.contains(&rb) {
                continue;
            }
            if let Some(inst) = b.get(rb) {
                candidates
                    .entry((inst.class(), inst.name()))
                    .or_default()
                    .push(rb);
            }
        }
        for &ra in &kids_a {
            if pairs.contains_key(&ra) {
                continue;
            }
            let Some(inst) = a.get(ra) else { continue };
            let Some(list) = candidates.get_mut(&(inst.class(), inst.name())) else {
                continue;
            };
            let id_a = unique_id(a, ra);
            let found = list.iter().position(|&rb| {
                !used.contains(&rb)
                    && match (id_a, unique_id(b, rb)) {
                        (Some(x), Some(y)) => x == y,
                        _ => true,
                    }
            });
            if let Some(i) = found {
                let rb = list.remove(i);
                used.insert(rb);
                pairs.insert(ra, rb);
                queue.push_back((Some(ra), Some(rb)));
            }
        }
    }
    pairs
}

fn unique_id(dom: &WeakDom, r: Ref) -> Option<UniqueId> {
    match dom.get(r)?.properties().get("UniqueId") {
        Some(Variant::UniqueId(id)) => Some(*id),
        _ => None,
    }
}

fn children(dom: &WeakDom, parent: Option<Ref>) -> Vec<Ref> {
    match parent {
        None => dom.root_refs().to_vec(),
        Some(p) => dom
            .get(p)
            .map(|i| i.children().to_vec())
            .unwrap_or_default(),
    }
}

/// Ids occurring exactly once in `dom`, with their instance.
fn unique_ids(dom: &WeakDom) -> HashMap<UniqueId, Ref> {
    let mut seen: HashMap<UniqueId, Option<Ref>> = HashMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        if let Some(id) = unique_id(dom, r) {
            seen.entry(id)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(r));
        }
        if let Some(inst) = dom.get(r) {
            stack.extend(inst.children().iter().rev());
        }
    }
    seen.into_iter()
        .filter_map(|(id, r)| r.map(|r| (id, r)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Instance;

    fn uid(n: u32) -> Variant {
        Variant::UniqueId(UniqueId {
            index: n,
            time: 1,
            random: 7,
        })
    }

    fn add(dom: &mut WeakDom, r: u32, name: &str, parent: Option<u32>, id: Option<u32>) -> Ref {
        let r = Ref::new(r);
        let mut i = Instance::new(r, "Part", name);
        if let Some(id) = id {
            i.properties_mut().insert("UniqueId".into(), uid(id));
        }
        dom.insert(i);
        if let Some(p) = parent {
            dom.set_parent(r, Some(Ref::new(p)));
        }
        r
    }

    #[test]
    fn unique_id_matches_across_refs() {
        let mut a = WeakDom::new();
        let mut b = WeakDom::new();
        let ra = add(&mut a, 1, "X", None, Some(5));
        let rb = add(&mut b, 101, "Y", None, Some(5));
        let m = match_doms(&a, &b, false);
        assert_eq!(m.get(&ra), Some(&rb));
    }

    #[test]
    fn duplicate_ids_are_ignored() {
        let mut a = WeakDom::new();
        let mut b = WeakDom::new();
        add(&mut a, 1, "X", None, Some(5));
        add(&mut a, 2, "X", None, Some(5));
        add(&mut b, 101, "X", None, Some(5));
        assert!(match_doms(&a, &b, false).is_empty());
    }

    #[test]
    fn positional_match_by_occurrence() {
        let mut a = WeakDom::new();
        let mut b = WeakDom::new();
        add(&mut a, 1, "W", None, Some(1));
        let a1 = add(&mut a, 2, "P", Some(1), None);
        let a2 = add(&mut a, 3, "P", Some(1), None);
        add(&mut b, 101, "W", None, Some(1));
        let b1 = add(&mut b, 102, "P", Some(101), None);
        let b2 = add(&mut b, 103, "P", Some(101), None);
        let m = match_doms(&a, &b, true);
        assert_eq!(m.get(&a1), Some(&b1));
        assert_eq!(m.get(&a2), Some(&b2));
    }

    #[test]
    fn differing_ids_never_pair_positionally() {
        let mut a = WeakDom::new();
        let mut b = WeakDom::new();
        let ra = add(&mut a, 1, "P", None, Some(1));
        add(&mut b, 101, "P", None, Some(2));
        let m = match_doms(&a, &b, true);
        assert!(!m.contains_key(&ra));
    }
}
