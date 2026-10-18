//! The instances of all three inputs, as one table of "the same instance
//! on each side".

use std::collections::HashMap;

use super::identity::match_doms;
use super::Side;
use crate::{Ref, WeakDom};

fn tree_order(dom: &WeakDom) -> Vec<Ref> {
    let mut order = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        order.push(r);
        if let Some(inst) = dom.get(r) {
            stack.extend(inst.children().iter().rev());
        }
    }
    order
}

/// One instance across the inputs: its referent on each side that has it,
/// and its referent in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) base: Option<Ref>,
    pub(crate) ours: Option<Ref>,
    pub(crate) theirs: Option<Ref>,
    /// Ours' referent when ours has the instance; otherwise one reserved
    /// from `ours.next_ref()` upward.
    pub(crate) out: Ref,
}

impl Node {
    pub(crate) fn on(&self, side: Side) -> Option<Ref> {
        match side {
            Side::Base => self.base,
            Side::Ours => self.ours,
            Side::Theirs => self.theirs,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Nodes {
    pub(crate) list: Vec<Node>,
    /// Per side (indexed `Side as usize`: Base 0, Ours 1, Theirs 2), referent
    /// to index into `list`.
    index: [HashMap<Ref, usize>; 3],
}

impl Nodes {
    /// Builds the table: base↔ours and base↔theirs with
    /// `identity::match_doms(_, _, true)`; then the ours instances base lacks
    /// paired with the theirs instances base lacks by `UniqueId` only
    /// (`match_doms(ours, theirs, false)` filtered to those). Every instance
    /// of every side is in exactly one node. List order and reserved
    /// referents are deterministic: ours' instances in tree order (roots in
    /// order, depth first, children in order), then theirs' remaining ones
    /// in tree order, then base's remaining ones in tree order; nodes
    /// without ours take `ours.next_ref()`, `+1`, … in that list order.
    pub(crate) fn new(base: &WeakDom, ours: &WeakDom, theirs: &WeakDom) -> Nodes {
        let base_to_ours = match_doms(base, ours, true);
        let base_to_theirs = match_doms(base, theirs, true);
        let ours_to_base: HashMap<Ref, Ref> = base_to_ours.iter().map(|(&b, &o)| (o, b)).collect();
        let theirs_to_base: HashMap<Ref, Ref> =
            base_to_theirs.iter().map(|(&b, &t)| (t, b)).collect();
        let ours_to_theirs: HashMap<Ref, Ref> = match_doms(ours, theirs, false)
            .into_iter()
            .filter(|(o, t)| !ours_to_base.contains_key(o) && !theirs_to_base.contains_key(t))
            .collect();

        let mut nodes = Nodes::default();
        let mut next = ours.next_ref();
        let mut push = |nodes: &mut Nodes, b: Option<Ref>, o: Option<Ref>, t: Option<Ref>| {
            let out = o.unwrap_or_else(|| {
                let r = Ref::new(next);
                next += 1;
                r
            });
            let i = nodes.list.len();
            nodes.list.push(Node {
                base: b,
                ours: o,
                theirs: t,
                out,
            });
            for (side, r) in [(Side::Base, b), (Side::Ours, o), (Side::Theirs, t)] {
                if let Some(r) = r {
                    nodes.index[side as usize].insert(r, i);
                }
            }
        };

        for o in tree_order(ours) {
            let b = ours_to_base.get(&o).copied();
            let t = match b {
                Some(b) => base_to_theirs.get(&b),
                None => ours_to_theirs.get(&o),
            }
            .copied();
            push(&mut nodes, b, Some(o), t);
        }
        for t in tree_order(theirs) {
            if nodes.index[Side::Theirs as usize].contains_key(&t) {
                continue;
            }
            let b = theirs_to_base.get(&t).copied();
            push(&mut nodes, b, None, Some(t));
        }
        for b in tree_order(base) {
            if nodes.index[Side::Base as usize].contains_key(&b) {
                continue;
            }
            push(&mut nodes, Some(b), None, None);
        }
        nodes
    }

    /// The node `referent` on `side` belongs to.
    pub(crate) fn of(&self, side: Side, referent: Ref) -> Option<&Node> {
        self.index[side as usize]
            .get(&referent)
            .map(|&i| &self.list[i])
    }
}
