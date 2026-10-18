//! Sibling order under one output parent.

use std::collections::HashSet;

use super::Index;
use crate::merge::Side;

/// The node indices of `side`'s children of node `parent` (its roots when
/// `None`), in that side's order.
fn side_list(ix: &Index<'_>, side: Side, parent: Option<usize>) -> Vec<usize> {
    let dom = ix.dom(side);
    let refs: &[crate::Ref] = match parent {
        None => dom.root_refs(),
        Some(p) => match ix.nodes.list[p].on(side).and_then(|r| dom.get(r)) {
            Some(inst) => inst.children(),
            None => &[],
        },
    };
    refs.iter().filter_map(|&r| ix.index(side, r)).collect()
}

/// `members` (every node landing under `parent`) in output order: ours'
/// order first, then theirs' others each after its nearest preceding
/// placed sibling in theirs' order, then any left over in `members` order.
pub(super) fn children(ix: &Index<'_>, parent: Option<usize>, members: &[usize]) -> Vec<usize> {
    let wanted: HashSet<usize> = members.iter().copied().collect();
    let mut placed: HashSet<usize> = HashSet::new();
    let mut result: Vec<usize> = Vec::new();
    for i in side_list(ix, Side::Ours, parent) {
        if wanted.contains(&i) && placed.insert(i) {
            result.push(i);
        }
    }
    let theirs = side_list(ix, Side::Theirs, parent);
    for (k, &i) in theirs.iter().enumerate() {
        if !wanted.contains(&i) || placed.contains(&i) {
            continue;
        }
        let at = theirs[..k]
            .iter()
            .rev()
            .find(|j| placed.contains(j))
            .and_then(|j| result.iter().position(|r| r == j))
            .map_or(0, |pos| pos + 1);
        result.insert(at, i);
        placed.insert(i);
    }
    for &i in members {
        if placed.insert(i) {
            result.push(i);
        }
    }
    result
}
