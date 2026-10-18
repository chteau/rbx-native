//! Which instances the merge keeps.

use super::Index;
use crate::merge::{Conflict, Resolution, Side, Target};
use crate::{Instance, Ref, Variant};

/// Whether `a` on base and `b` on `side` hold the same content, `Ref`s
/// compared by the node they name.
fn same_content(ix: &Index<'_>, a: &Instance, side: Side, b: &Instance) -> bool {
    if a.name() != b.name() || a.class() != b.class() {
        return false;
    }
    let (pa, pb) = (a.properties(), b.properties());
    pa.len() == pb.len()
        && pa.iter().all(|(key, va)| match (va, pb.get(key)) {
            (Variant::Ref(ra), Some(Variant::Ref(rb))) => {
                let na = ix.index(Side::Base, *ra);
                na.is_some() && na == ix.index(side, *rb)
            }
            (va, Some(vb)) => va == vb,
            (_, None) => false,
        })
}

/// Per node, whether `side` changed it or anything under it (added,
/// changed, moved, moved in or out, or removed something beneath it).
fn changed(ix: &Index<'_>, side: Side) -> Vec<bool> {
    let list = &ix.nodes.list;
    let mut seeds: Vec<usize> = Vec::new();
    for (i, node) in list.iter().enumerate() {
        let base_parent = ix.parent(Side::Base, i).flatten();
        let Some(r) = node.on(side) else {
            seeds.extend(base_parent);
            continue;
        };
        let Some(b) = node.base else {
            seeds.push(i);
            continue;
        };
        let moved = ix.parent(side, i) != ix.parent(Side::Base, i);
        let differs = match (ix.dom(Side::Base).get(b), ix.dom(side).get(r)) {
            (Some(a), Some(c)) => !same_content(ix, a, side, c),
            _ => true,
        };
        if moved || differs {
            seeds.push(i);
        }
        if moved {
            seeds.extend(base_parent);
        }
    }
    let mut mark = vec![false; list.len()];
    for seed in seeds {
        if list[seed].on(side).is_none() {
            continue;
        }
        let mut cur = Some(seed);
        while let Some(c) = cur {
            if mark[c] {
                break;
            }
            mark[c] = true;
            cur = ix.parent(side, c).flatten();
        }
    }
    mark
}

/// Per node, whether it is in the output; pushes the `Existence` conflicts.
pub(super) fn survive(
    ix: &Index<'_>,
    choose: &dyn Fn(Ref, &Target) -> Option<Resolution>,
    conflicts: &mut Vec<Conflict>,
) -> Vec<bool> {
    let list = &ix.nodes.list;
    let changed_ours = changed(ix, Side::Ours);
    let changed_theirs = changed(ix, Side::Theirs);
    let mut kept = vec![false; list.len()];
    for (i, &node) in list.iter().enumerate() {
        let (keeper, removed_by, changed) = match (node.base, node.ours, node.theirs) {
            (_, Some(_), Some(_)) => {
                kept[i] = true;
                continue;
            }
            (None, o, _) => {
                let side = if o.is_some() {
                    Side::Ours
                } else {
                    Side::Theirs
                };
                kept[i] = match ix.parent(side, i) {
                    Some(Some(p)) => kept[p],
                    Some(None) => true,
                    None => false,
                };
                continue;
            }
            (Some(_), None, None) => continue,
            (Some(_), Some(_), None) => (Side::Ours, Side::Theirs, &changed_ours),
            (Some(_), None, Some(_)) => (Side::Theirs, Side::Ours, &changed_theirs),
        };
        let parent = ix.parent(keeper, i).flatten();
        let inherited = parent.filter(|&p| {
            let pn = list[p];
            pn.base.is_some() && pn.on(keeper).is_some() && pn.on(removed_by).is_none()
        });
        if let Some(p) = inherited {
            kept[i] = kept[p];
            continue;
        }
        if !changed[i] {
            continue;
        }
        let target = Target::Existence { removed_by };
        kept[i] = match choose(node.out, &target) {
            Some(Resolution::Theirs) => node.theirs.is_some(),
            _ => node.ours.is_some(),
        };
        let value = |side: Side| ix.path(side, i).map(Variant::String);
        let values = [value(Side::Base), value(Side::Ours), value(Side::Theirs)];
        ix.push_conflict(conflicts, i, target, values);
    }
    kept
}
