//! Builds the merged tree: which instances survive, where each hangs, in
//! what order, and what each holds.

mod order;
mod survive;

use std::collections::HashMap;

use super::nodes::Nodes;
use super::props::{merge_content, Content, Sides};
use super::{Conflict, Resolution, Side, Target};
use crate::{Instance, Ref, Variant, WeakDom};

/// The three inputs and the node table, addressed by node index.
pub(super) struct Index<'a> {
    pub(super) nodes: &'a Nodes,
    doms: [&'a WeakDom; 3],
    by_out: HashMap<Ref, usize>,
}

impl<'a> Index<'a> {
    pub(super) fn dom(&self, side: Side) -> &'a WeakDom {
        self.doms[side as usize]
    }

    pub(super) fn index(&self, side: Side, r: Ref) -> Option<usize> {
        let node = self.nodes.of(side, r)?;
        self.by_out.get(&node.out).copied()
    }

    pub(super) fn inst(&self, side: Side, i: usize) -> Option<&'a Instance> {
        self.dom(side).get(self.nodes.list[i].on(side)?)
    }

    /// The node's parent on `side`: `None` when `side` lacks the node,
    /// `Some(None)` when it is a root there.
    pub(super) fn parent(&self, side: Side, i: usize) -> Option<Option<usize>> {
        let r = self.nodes.list[i].on(side)?;
        Some(self.dom(side).parent(r).and_then(|p| self.index(side, p)))
    }

    fn ref_path(&self, side: Side, r: Ref) -> String {
        let dom = self.dom(side);
        let mut names = Vec::new();
        let mut cur = Some(r);
        while let Some(c) = cur {
            let Some(inst) = dom.get(c) else { break };
            names.push(inst.name());
            cur = dom.parent(c);
        }
        names.reverse();
        names.join(".")
    }

    pub(super) fn path(&self, side: Side, i: usize) -> Option<String> {
        Some(self.ref_path(side, self.nodes.list[i].on(side)?))
    }

    fn parent_path(&self, side: Side, i: usize) -> Option<Variant> {
        let r = self.nodes.list[i].on(side)?;
        let path = match self.dom(side).parent(r) {
            Some(p) => self.ref_path(side, p),
            None => String::new(),
        };
        Some(Variant::String(path))
    }

    pub(super) fn push_conflict(
        &self,
        conflicts: &mut Vec<Conflict>,
        i: usize,
        target: Target,
        [base, ours, theirs]: [Option<Variant>; 3],
    ) {
        let order = [Side::Ours, Side::Theirs, Side::Base];
        let inst = order.iter().find_map(|&s| self.inst(s, i));
        let path = order
            .iter()
            .find_map(|&s| self.path(s, i))
            .unwrap_or_default();
        let unique_id =
            order
                .iter()
                .find_map(|&s| match self.inst(s, i)?.properties().get("UniqueId")? {
                    Variant::UniqueId(id) => Some(*id),
                    _ => None,
                });
        conflicts.push(Conflict {
            id: conflicts.len(),
            target,
            instance: self.nodes.list[i].out,
            class: inst.map(|x| x.class().to_owned()).unwrap_or_default(),
            name: inst.map(|x| x.name().to_owned()).unwrap_or_default(),
            path,
            unique_id,
            base,
            ours,
            theirs,
            resolution: None,
        });
    }
}

/// Whether hanging `i` under `p` would put it under itself.
fn under_itself(parent: &[Option<usize>], i: usize, p: usize) -> bool {
    let mut cur = Some(p);
    let mut steps = 0;
    while let Some(c) = cur {
        if c == i {
            return true;
        }
        steps += 1;
        if steps > parent.len() {
            return true;
        }
        cur = parent[c];
    }
    false
}

/// Per node, its output parent (meaningful only for kept nodes).
fn parents(
    ix: &Index<'_>,
    kept: &[bool],
    choose: &dyn Fn(Ref, &Target) -> Option<Resolution>,
    conflicts: &mut Vec<Conflict>,
) -> Vec<Option<usize>> {
    let n = kept.len();
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut theirs_moves: Vec<usize> = Vec::new();
    let in_output = |p: Option<Option<usize>>| match p {
        Some(Some(j)) => kept[j],
        Some(None) => true,
        None => false,
    };
    let settle = |r: Option<Resolution>, b, o, t| match r {
        Some(Resolution::Base) => b,
        Some(Resolution::Theirs) => t,
        _ => o,
    };
    let values = |i| [Side::Base, Side::Ours, Side::Theirs].map(|s| ix.parent_path(s, i));
    for i in (0..n).filter(|&i| kept[i]) {
        let b = ix.parent(Side::Base, i);
        let o = ix.parent(Side::Ours, i);
        let t = ix.parent(Side::Theirs, i);
        let chosen = match (o, t) {
            (Some(_), Some(_)) if o == t => o,
            (Some(_), Some(_)) if b.is_some() && o == b => {
                theirs_moves.push(i);
                t
            }
            (Some(_), Some(_)) if b.is_some() && t == b => o,
            (Some(_), Some(_)) => {
                let r = choose(ix.nodes.list[i].out, &Target::Parent);
                ix.push_conflict(conflicts, i, Target::Parent, values(i));
                settle(r, b, o, t)
            }
            (Some(_), None) => o,
            _ => t,
        };
        parent[i] = [chosen, o, t]
            .into_iter()
            .find(|&p| in_output(p))
            .flatten()
            .flatten();
    }
    for i in theirs_moves {
        let Some(p) = parent[i] else { continue };
        if !under_itself(&parent, i, p) {
            continue;
        }
        let r = choose(ix.nodes.list[i].out, &Target::Parent);
        ix.push_conflict(conflicts, i, Target::Parent, values(i));
        let picked = settle(
            r,
            ix.parent(Side::Base, i),
            ix.parent(Side::Ours, i),
            ix.parent(Side::Theirs, i),
        );
        parent[i] = [picked, ix.parent(Side::Ours, i)]
            .into_iter()
            .find(|&p| in_output(p))
            .flatten()
            .flatten();
    }
    // Whatever the resolutions, the output must stay a tree.
    for i in 0..n {
        if let Some(p) = parent[i] {
            if under_itself(&parent, i, p) {
                parent[i] = None;
            }
        }
    }
    parent
}

/// The merged DOM and every conflict met building it, numbered in the
/// order met (`id` = index, `resolution: None`). `choose(instance, target)`
/// is the resolution for the conflict on output referent `instance`;
/// `None` takes ours.
///
/// Survival, per node: in ours and theirs → kept; added by one side (no
/// base, absent on the other) → kept, unless its parent is not in the
/// output (then dropped silently); removed by both → gone. Removed by one
/// side only → gone, unless the other side changed it — its content
/// differs from base, it moved, or any instance under it on that side was
/// added, changed, moved in or out — which is `Target::Existence {
/// removed_by }` (one conflict, at the topmost removed instance only, none
/// for its descendants): ours keeps ours' state, `Theirs` takes theirs'.
/// A kept instance whose removed-by-the-other-side subtree is restored
/// brings its whole subtree from the keeping side.
///
/// Parent, per kept node, compared by node: the same rule as a property
/// (`Target::Parent` when both moved it apart); theirs' move is also a
/// `Target::Parent` conflict when the new parent is the instance itself or
/// under it in the output. A parent not in the output falls back to the
/// other side's parent, then to a root.
///
/// Children order under each output parent: ours' order of the children
/// that land there, then every other child placed right after its nearest
/// preceding sibling in theirs' order that is already placed (at the front
/// when there is none), in theirs' order. Roots the same way.
///
/// Content comes from `props::merge_content` with every side the node has
/// (base included only when the instance exists on base), `out` mapping
/// through `nodes`, and its `Found`s becoming `Conflict`s for that node.
/// Output instances are inserted with `Instance::new(node.out, ..)`, and the
/// output's `raise_next_ref` is set past every reserved referent. The
/// output's change log is drained before returning.
pub(crate) fn build(
    base: &WeakDom,
    ours: &WeakDom,
    theirs: &WeakDom,
    nodes: &Nodes,
    choose: &dyn Fn(Ref, &Target) -> Option<Resolution>,
) -> (WeakDom, Vec<Conflict>) {
    let ix = Index {
        nodes,
        doms: [base, ours, theirs],
        by_out: nodes
            .list
            .iter()
            .enumerate()
            .map(|(i, n)| (n.out, i))
            .collect(),
    };
    let list = &nodes.list;
    let mut conflicts = Vec::new();
    let kept = survive::survive(&ix, choose, &mut conflicts);
    let parent = parents(&ix, &kept, choose, &mut conflicts);

    let out_ref = |side: Side, r: Ref| ix.index(side, r).filter(|&j| kept[j]).map(|j| list[j].out);
    let mut contents: Vec<Option<Content>> = vec![None; list.len()];
    for i in (0..list.len()).filter(|&i| kept[i]) {
        let node = list[i];
        let sides = Sides {
            base: ix.inst(Side::Base, i),
            ours: ix.inst(Side::Ours, i),
            theirs: ix.inst(Side::Theirs, i),
        };
        let pick = |t: &Target| choose(node.out, t);
        let (content, found) = merge_content(&sides, &out_ref, &pick);
        for f in found {
            ix.push_conflict(&mut conflicts, i, f.target, [f.base, f.ours, f.theirs]);
        }
        contents[i] = Some(content);
    }
    for c in &mut conflicts {
        let content = ix
            .by_out
            .get(&c.instance)
            .and_then(|&i| contents[i].as_ref());
        if let Some(content) = content {
            c.class.clone_from(&content.class);
            c.name.clone_from(&content.name);
        }
    }

    let mut members: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for i in (0..list.len()).filter(|&i| kept[i]) {
        members.entry(parent[i]).or_default().push(i);
    }
    let mut dom = WeakDom::new();
    let roots = members
        .get(&None)
        .map(|m| order::children(&ix, None, m))
        .unwrap_or_default();
    let mut inserted = vec![false; list.len()];
    let firsts = roots.iter().copied();
    let rest = (0..list.len()).filter(|&i| kept[i]);
    for i in firsts.chain(rest) {
        if inserted[i] {
            continue;
        }
        let Some(content) = contents[i].take() else {
            continue;
        };
        inserted[i] = true;
        let mut inst = Instance::new(list[i].out, content.class, content.name);
        *inst.properties_mut() = content.properties;
        dom.insert(inst);
    }
    // Parents before children, so each `set_parent` appends in final order.
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(p) = stack.pop() {
        let Some(kids) = members.get(&Some(p)) else {
            continue;
        };
        let ordered = order::children(&ix, Some(p), kids);
        for &k in &ordered {
            dom.set_parent(list[k].out, Some(list[p].out));
        }
        stack.extend(ordered.into_iter().rev());
    }

    let floor = list
        .iter()
        .map(|n| n.out.value().saturating_add(1))
        .max()
        .unwrap_or(0)
        .max(ours.next_ref());
    dom.raise_next_ref(floor);
    dom.take_changes();
    (dom, conflicts)
}
