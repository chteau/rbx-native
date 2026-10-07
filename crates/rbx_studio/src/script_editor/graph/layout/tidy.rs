//! Tidy: lay the graph out the way the code reads. Sugiyama-style and
//! deterministic (only ordered maps; the one thing read from the current
//! positions is the order items stack in).
//!
//! An *item* is an event or start node with its whole run (every node
//! reachable through run-order wires) and the value nodes feeding it. Run
//! nodes sit in columns by longest run-order path from the root; the main
//! continuation of a run stays on its row ("lane") and every nested run
//! (a branch's arm, a loop body) gets a lane below. Value nodes sit in
//! columns left of the node that reads them, level with its pin. Items are
//! stacked top to bottom, so nothing overlaps.

use std::collections::{BTreeMap, BTreeSet};

use super::super::{Graph, NodeId};
use super::{group_title, pin, rect, Rect, Side};

const ORIGIN: [f32; 2] = [60.0, 60.0];
/// Between columns: room for wires.
const GAP_X: f32 = 70.0;
/// Breathing room kept round every node.
const MARGIN: f32 = 14.0;
const LANE_GAP: f32 = 30.0;
const ITEM_GAP: f32 = 80.0;
/// Between a group's frame and the nodes it was refit round.
const PAD: f32 = 24.0;

/// Packs rectangles: a node asked for a spot slides down past whatever is
/// in the way.
#[derive(Default)]
struct Packer {
    placed: Vec<Rect>,
}

impl Packer {
    fn bottom(&self) -> Option<f32> {
        self.placed.iter().map(|r| r.y + r.h).reduce(f32::max)
    }

    /// Puts node `id`, whose x is already set, at `y` or the first free
    /// spot below it.
    fn put(&mut self, graph: &mut Graph, id: NodeId, mut y: f32) {
        let Some(node) = graph.node(id) else { return };
        let mut at = rect(graph, node);
        loop {
            at.y = y;
            let wall = Rect {
                x: at.x - MARGIN,
                y: at.y - MARGIN,
                w: at.w + 2.0 * MARGIN,
                h: at.h + 2.0 * MARGIN,
            };
            match self.placed.iter().find(|r| r.intersects(&wall)) {
                Some(r) => y = r.y + r.h + MARGIN + MARGIN,
                None => break,
            }
        }
        if let Some(node) = graph.node_mut(id) {
            node.y = y;
        }
        self.placed.push(at);
    }
}

/// Run-order and value wiring, read once.
struct Wiring {
    touches: BTreeSet<NodeId>,
    has_exec_in: BTreeSet<NodeId>,
    has_wire_out: BTreeSet<NodeId>,
    /// Per node: (pin height, target, is the main continuation), top pin first.
    exec_out: BTreeMap<NodeId, Vec<(f32, NodeId, bool)>>,
    /// Per node: the value nodes wired into it, by input pin.
    inputs: BTreeMap<NodeId, Vec<NodeId>>,
}

impl Wiring {
    fn read(graph: &Graph) -> Wiring {
        let mut w = Wiring {
            touches: BTreeSet::new(),
            has_exec_in: BTreeSet::new(),
            has_wire_out: BTreeSet::new(),
            exec_out: BTreeMap::new(),
            inputs: BTreeMap::new(),
        };
        let mut by_pin: BTreeMap<NodeId, Vec<(f32, NodeId, NodeId)>> = BTreeMap::new();
        for wire in &graph.wires {
            let (a, b) = (wire.from.node, wire.to.node);
            if a == b {
                continue;
            }
            w.has_wire_out.insert(a);
            if graph.is_exec(&wire.from) {
                w.touches.extend([a, b]);
                w.has_exec_in.insert(b);
                let y = pin(graph, &wire.from, Side::Output).map_or(0.0, |p| p[1]);
                let main = matches!(wire.from.pin.as_str(), "" | "Completed");
                w.exec_out.entry(a).or_default().push((y, b, main));
            } else {
                let y = pin(graph, &wire.to, Side::Input).map_or(0.0, |p| p[1]);
                by_pin.entry(b).or_default().push((y, a, a));
            }
        }
        for outs in w.exec_out.values_mut() {
            outs.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        }
        for (b, mut ins) in by_pin {
            ins.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
            w.inputs.insert(b, ins.into_iter().map(|i| i.1).collect());
        }
        w
    }

    /// A pure value node: wired only by value wires, and not an event.
    fn is_value(&self, graph: &Graph, id: NodeId) -> bool {
        !self.touches.contains(&id) && !graph.kind_of(id).is_some_and(|k| k.is_event())
    }
}

#[derive(Default)]
struct Item {
    /// Run nodes in the order the walk met them.
    run: Vec<NodeId>,
    lane: BTreeMap<NodeId, usize>,
    layer: BTreeMap<NodeId, usize>,
    values: BTreeSet<NodeId>,
    lanes: usize,
}

struct Walk<'a> {
    wiring: &'a Wiring,
    claimed: &'a mut BTreeSet<NodeId>,
    item: Item,
    on_stack: BTreeSet<NodeId>,
    /// Run-order edges that do not close a loop.
    edges: Vec<(NodeId, NodeId)>,
}

impl Walk<'_> {
    /// Nested arms get lanes first (so they sit above later lanes), the
    /// main continuation carries on in this lane.
    fn go(&mut self, n: NodeId, lane: usize) {
        self.claimed.insert(n);
        self.item.lane.insert(n, lane);
        self.item.run.push(n);
        self.on_stack.insert(n);
        let outs = self.wiring.exec_out.get(&n).cloned().unwrap_or_default();
        for main in [false, true] {
            for &(_, to, is_main) in outs.iter().filter(|o| o.2 == main) {
                if self.item.lane.contains_key(&to) {
                    if !self.on_stack.contains(&to) {
                        self.edges.push((n, to));
                    }
                } else if !self.claimed.contains(&to) {
                    self.edges.push((n, to));
                    let lane = if is_main {
                        lane
                    } else {
                        self.item.lanes += 1;
                        self.item.lanes
                    };
                    self.go(to, lane);
                }
            }
        }
        self.on_stack.remove(&n);
    }

    fn feed(&mut self, graph: &Graph, n: NodeId) {
        for &src in self.wiring.inputs.get(&n).into_iter().flatten() {
            if !self.claimed.contains(&src) && self.wiring.is_value(graph, src) {
                self.claimed.insert(src);
                self.item.values.insert(src);
                self.feed(graph, src);
            }
        }
    }
}

fn build_item(
    graph: &Graph,
    wiring: &Wiring,
    claimed: &mut BTreeSet<NodeId>,
    root: NodeId,
) -> Item {
    let mut walk = Walk {
        wiring,
        claimed,
        item: Item::default(),
        on_stack: BTreeSet::new(),
        edges: Vec::new(),
    };
    walk.go(root, 0);
    for n in walk.item.run.clone() {
        walk.feed(graph, n);
    }
    let Walk {
        mut item, edges, ..
    } = walk;
    item.lanes += 1;
    for &n in &item.run {
        item.layer.insert(n, 0);
    }
    for _ in 0..=item.run.len() {
        let mut changed = false;
        for &(a, b) in &edges {
            let next = item.layer[&a] + 1;
            if item.layer[&b] < next {
                item.layer.insert(b, next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    item
}

/// Value consumers of `v` within the item.
fn consumers(graph: &Graph, item: &Item, v: NodeId) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = graph
        .wires
        .iter()
        .filter(|w| w.from.node == v && w.to.node != v && !graph.is_exec(&w.from))
        .map(|w| w.to.node)
        .filter(|c| item.lane.contains_key(c) || item.values.contains(c))
        .collect();
    out.sort();
    out.dedup();
    out
}

struct Grid {
    col: BTreeMap<NodeId, i64>,
    owner: BTreeMap<NodeId, usize>,
}

/// Column and owning lane of every value node: left of its leftmost
/// consumer, and in the first lane that reads it.
fn grid(graph: &Graph, item: &Item, layer_col: &[i64]) -> Grid {
    let mut g = Grid {
        col: BTreeMap::new(),
        owner: BTreeMap::new(),
    };
    fn settle(graph: &Graph, item: &Item, layer_col: &[i64], g: &mut Grid, v: NodeId) {
        if g.col.contains_key(&v) {
            return;
        }
        g.col.insert(v, 0);
        g.owner.insert(v, 0);
        let (mut col, mut owner) = (i64::MAX, usize::MAX);
        for c in consumers(graph, item, v) {
            let (cc, co) = match item.lane.get(&c) {
                Some(&lane) => (layer_col[item.layer[&c]], lane),
                None => {
                    settle(graph, item, layer_col, g, c);
                    (g.col[&c], g.owner[&c])
                }
            };
            col = col.min(cc - 1);
            owner = owner.min(co);
        }
        g.col
            .insert(v, if col == i64::MAX { 0 } else { col.max(0) });
        g.owner
            .insert(v, if owner == usize::MAX { 0 } else { owner });
    }
    for &v in &item.values {
        settle(graph, item, layer_col, &mut g, v);
    }
    g
}

/// Run columns, spaced so each layer has room for the deepest chain of
/// value nodes feeding it.
fn layer_cols(wiring: &Wiring, item: &Item) -> Vec<i64> {
    fn chain(wiring: &Wiring, item: &Item, memo: &mut BTreeMap<NodeId, i64>, n: NodeId) -> i64 {
        if let Some(&d) = memo.get(&n) {
            return d;
        }
        memo.insert(n, 0);
        let d = wiring
            .inputs
            .get(&n)
            .into_iter()
            .flatten()
            .filter(|s| item.values.contains(s))
            .map(|&s| 1 + chain(wiring, item, memo, s))
            .max()
            .unwrap_or(0);
        memo.insert(n, d);
        d
    }
    let layers = item.layer.values().max().map_or(0, |m| m + 1);
    let mut need = vec![0i64; layers];
    let mut memo = BTreeMap::new();
    for &n in &item.run {
        let l = item.layer[&n];
        need[l] = need[l].max(chain(wiring, item, &mut memo, n));
    }
    let mut at = -1;
    need.iter()
        .map(|n| {
            at += n + 1;
            at
        })
        .collect()
}

/// Mean height a value node should sit at to be level with the pins it
/// feeds, over the consumers already placed.
fn level_with_consumers(graph: &Graph, placed: &BTreeSet<NodeId>, v: NodeId) -> Option<f32> {
    let node = graph.node(v)?;
    let mut sum = 0.0;
    let mut n = 0;
    for w in graph
        .wires
        .iter()
        .filter(|w| w.from.node == v && !graph.is_exec(&w.from) && placed.contains(&w.to.node))
    {
        let (Some(to), Some(from)) = (
            pin(graph, &w.to, Side::Input),
            pin(graph, &w.from, Side::Output),
        ) else {
            continue;
        };
        sum += to[1] - (from[1] - node.y);
        n += 1;
    }
    (n > 0).then(|| sum / n as f32)
}

/// Where a run node should sit to put its run-order pin level with the one
/// that feeds it, preferring a predecessor in its own lane.
fn level_with_pred(
    graph: &Graph,
    item: &Item,
    placed: &BTreeSet<NodeId>,
    n: NodeId,
) -> Option<f32> {
    let node = graph.node(n)?;
    let lane = item.lane[&n];
    graph
        .wires
        .iter()
        .filter(|w| w.to.node == n && graph.is_exec(&w.from) && placed.contains(&w.from.node))
        .min_by_key(|w| (item.lane.get(&w.from.node) != Some(&lane), w.from.node))
        .and_then(|w| {
            let from = pin(graph, &w.from, Side::Output)?;
            let to = pin(graph, &w.to, Side::Input)?;
            Some(from[1] - (to[1] - node.y))
        })
}

fn lay_item(graph: &mut Graph, wiring: &Wiring, item: &Item, top: f32) -> f32 {
    let layer_col = layer_cols(wiring, item);
    let g = grid(graph, item, &layer_col);
    let col_of = |n: NodeId| match item.lane.contains_key(&n) {
        true => layer_col[item.layer[&n]],
        false => g.col[&n],
    };
    // Column x from the widest node in each.
    let mut widths: BTreeMap<i64, f32> = BTreeMap::new();
    for &n in item.lane.keys().chain(&item.values) {
        if let Some(node) = graph.node(n) {
            let w = rect(graph, node).w;
            let e = widths.entry(col_of(n)).or_insert(0.0);
            *e = e.max(w);
        }
    }
    let mut x = ORIGIN[0];
    let mut colx: BTreeMap<i64, (f32, f32)> = BTreeMap::new();
    for i in 0..=widths.keys().max().copied().unwrap_or(0) {
        let w = widths.get(&i).copied().unwrap_or(0.0);
        colx.insert(i, (x, w));
        x += w + if w > 0.0 { GAP_X } else { 0.0 };
    }
    let mut packer = Packer::default();
    let mut placed: BTreeSet<NodeId> = BTreeSet::new();
    let mut cursor = top;
    for lane in 0..item.lanes {
        let mut run: Vec<NodeId> = item
            .run
            .iter()
            .copied()
            .filter(|n| item.lane[n] == lane)
            .collect();
        run.sort_by_key(|n| item.layer[n]);
        for n in run {
            let (cx, _) = colx[&col_of(n)];
            if let Some(node) = graph.node_mut(n) {
                node.x = cx;
            }
            let want = level_with_pred(graph, item, &placed, n).unwrap_or(cursor);
            packer.put(graph, n, want.max(cursor));
            placed.insert(n);
        }
        // Value nodes this lane owns, nearest the consumer first; within a
        // column, in the order of the height each wants (the barycentre).
        let mut mine: Vec<NodeId> = item
            .values
            .iter()
            .copied()
            .filter(|v| g.owner[v] == lane)
            .collect();
        let mut cols: Vec<i64> = mine.iter().map(|&v| g.col[&v]).collect();
        cols.sort_unstable_by(|a, b| b.cmp(a));
        cols.dedup();
        for c in cols {
            let (cx, cw) = colx[&c];
            let mut here: Vec<(f32, NodeId)> = mine
                .iter()
                .filter(|v| g.col[v] == c)
                .map(|&v| (level_with_consumers(graph, &placed, v).unwrap_or(cursor), v))
                .collect();
            here.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for (want, v) in here {
                let w = graph.node(v).map_or(0.0, |n| rect(graph, n).w);
                if let Some(node) = graph.node_mut(v) {
                    node.x = cx + cw - w;
                }
                packer.put(graph, v, want.max(cursor));
                placed.insert(v);
            }
            mine.retain(|v| g.col[v] != c);
        }
        cursor = cursor.max(packer.bottom().unwrap_or(cursor) + LANE_GAP);
    }
    packer.bottom().unwrap_or(top)
}

/// The nodes that start an item, in the order items stack: events, run
/// chains that nothing runs into, and nodes with nothing wired out.
fn roots(graph: &Graph, wiring: &Wiring) -> Vec<NodeId> {
    let mut roots: Vec<&crate::script_editor::graph::Node> = graph
        .nodes
        .iter()
        .filter(|n| {
            graph.kind_of(n.id).is_some_and(|k| k.is_event())
                || (wiring.touches.contains(&n.id) && !wiring.has_exec_in.contains(&n.id))
                || (!wiring.touches.contains(&n.id) && !wiring.has_wire_out.contains(&n.id))
        })
        .collect();
    roots.sort_by(|a, b| {
        a.y.total_cmp(&b.y)
            .then(a.x.total_cmp(&b.x))
            .then(a.id.cmp(&b.id))
    });
    roots.into_iter().map(|n| n.id).collect()
}

/// Lays every item out, stacked from the top left, then refits each group
/// round the nodes it held.
pub(crate) fn tidy(graph: &mut Graph) {
    let held: Vec<Vec<NodeId>> = (0..graph.groups.len())
        .map(|i| graph.nodes_within(i))
        .collect();
    let wiring = Wiring::read(graph);
    let mut claimed = BTreeSet::new();
    let mut top = ORIGIN[1];
    let mut next_root = roots(graph, &wiring).into_iter();
    loop {
        // Anything left (a run that only loops back on itself, say) starts
        // an item of its own, in the order it lay.
        let root = next_root.find(|r| !claimed.contains(r)).or_else(|| {
            let mut rest: Vec<_> = graph
                .nodes
                .iter()
                .filter(|n| !claimed.contains(&n.id))
                .collect();
            rest.sort_by(|a, b| {
                wiring
                    .is_value(graph, a.id)
                    .cmp(&wiring.is_value(graph, b.id))
                    .then(a.y.total_cmp(&b.y))
                    .then(a.x.total_cmp(&b.x))
                    .then(a.id.cmp(&b.id))
            });
            rest.first().map(|n| n.id)
        });
        let Some(root) = root else { break };
        let item = build_item(graph, &wiring, &mut claimed, root);
        top = lay_item(graph, &wiring, &item, top) + ITEM_GAP;
    }
    refit_groups(graph, &held);
}

fn refit_groups(graph: &mut Graph, held: &[Vec<NodeId>]) {
    for (i, ids) in held.iter().enumerate() {
        let frame = ids
            .iter()
            .filter_map(|&id| graph.node(id).map(|n| rect(graph, n)))
            .reduce(|a, b| a.union(&b));
        let Some(frame) = frame else { continue };
        let group = &mut graph.groups[i];
        let title = group_title(group).w + 24.0;
        group.x = (frame.x - PAD).round();
        group.y = (frame.y - PAD - 12.0).round();
        group.w = (frame.w + 2.0 * PAD).max(title).round();
        group.h = (frame.h + 2.0 * PAD + 12.0).round();
    }
}

/// Places the nodes not in `fixed` near what they are wired to, leaving
/// `fixed` exactly where it is. Nodes wired to nothing placed go below
/// everything.
pub(crate) fn place_new(graph: &mut Graph, fixed: &BTreeSet<NodeId>) {
    let mut packer = Packer::default();
    for node in graph.nodes.iter().filter(|n| fixed.contains(&n.id)) {
        packer.placed.push(rect(graph, node));
    }
    let mut placed: BTreeSet<NodeId> = fixed.clone();
    let mut pending: Vec<NodeId> = graph
        .nodes
        .iter()
        .map(|n| n.id)
        .filter(|id| !fixed.contains(id))
        .collect();
    loop {
        let before = pending.len();
        let mut rest = Vec::new();
        for id in pending {
            match anchor(graph, &placed, id) {
                Some((x, y)) => {
                    if let Some(node) = graph.node_mut(id) {
                        node.x = x;
                    }
                    packer.put(graph, id, y);
                    placed.insert(id);
                }
                None => rest.push(id),
            }
        }
        pending = rest;
        if pending.is_empty() || pending.len() == before {
            break;
        }
    }
    let left = packer
        .placed
        .iter()
        .map(|r| r.x)
        .reduce(f32::min)
        .unwrap_or(ORIGIN[0]);
    for id in pending {
        let y = packer.bottom().map_or(ORIGIN[1], |b| b + LANE_GAP);
        if let Some(node) = graph.node_mut(id) {
            node.x = left;
        }
        packer.put(graph, id, y);
    }
}

/// Where `id` wants to go by one placed neighbour: right of what runs into
/// it, left of what reads it, left of what it runs into, right of what
/// feeds it — level with the wire's pin in each case.
fn anchor(graph: &Graph, placed: &BTreeSet<NodeId>, id: NodeId) -> Option<(f32, f32)> {
    let me = graph.node(id)?;
    let size = rect(graph, me);
    let near = |exec: bool, incoming: bool| {
        graph.wires.iter().find_map(|w| {
            let (mine, theirs) = match incoming {
                true => (&w.to, &w.from),
                false => (&w.from, &w.to),
            };
            if mine.node != id || !placed.contains(&theirs.node) || graph.is_exec(&w.from) != exec {
                return None;
            }
            let (from, to) = (
                pin(graph, &w.from, Side::Output)?,
                pin(graph, &w.to, Side::Input)?,
            );
            let other = graph.node(theirs.node).map(|n| rect(graph, n))?;
            // The pin height on my side relative to my top.
            let off = match incoming {
                true => to[1] - me.y,
                false => from[1] - me.y,
            };
            let y = if incoming { from[1] } else { to[1] } - off;
            let x = match incoming {
                true => other.x + other.w + GAP_X,
                false => other.x - GAP_X - size.w,
            };
            Some((x, y))
        })
    };
    // Exec before value; what feeds me (runs into me / is read by me) is
    // to my left, what I feed is to my right.
    near(true, true)
        .or_else(|| near(false, false))
        .or_else(|| near(true, false))
        .or_else(|| near(false, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_editor::graph::{catalog, End, Group};

    fn add(g: &mut Graph, key: &str, at: [f32; 2]) -> NodeId {
        g.add(catalog::kind(key).unwrap(), at)
    }

    fn wire(g: &mut Graph, a: NodeId, ap: &str, b: NodeId, bp: &str) {
        g.connect(End::new(a, ap), End::new(b, bp)).unwrap();
    }

    /// Two events, a branch with both arms, values shared by two consumers.
    fn messy(jitter: f32) -> Graph {
        let mut g = Graph::default();
        let at = |i: f32| {
            [
                (i * 37.0 + jitter * 13.0) % 300.0,
                (i * 91.0 + jitter * 7.0) % 200.0,
            ]
        };
        let t = add(&mut g, "touched", at(1.0));
        let br = add(&mut g, "branch", at(2.0));
        let p1 = add(&mut g, "print", at(3.0));
        let p2 = add(&mut g, "print", at(4.0));
        let p3 = add(&mut g, "print", at(5.0));
        let valid = add(&mut g, "is_valid", at(6.0));
        let num = add(&mut g, "number", at(7.0));
        let neg = add(&mut g, "negate", at(8.0));
        let s = add(&mut g, "start", [500.0 - jitter, 500.0]);
        let p4 = add(&mut g, "print", at(9.0));
        wire(&mut g, t, "", br, "");
        wire(&mut g, br, "True", p1, "");
        wire(&mut g, br, "False", p2, "");
        wire(&mut g, br, "Completed", p3, "");
        wire(&mut g, valid, "Result", br, "Condition");
        wire(&mut g, num, "Result", neg, "Value");
        wire(&mut g, neg, "Result", p1, "Value");
        wire(&mut g, num, "Result", p2, "Value");
        wire(&mut g, s, "", p4, "");
        g
    }

    fn overlaps(g: &Graph) -> bool {
        let rs: Vec<Rect> = g.nodes.iter().map(|n| rect(g, n)).collect();
        (0..rs.len()).any(|i| (i + 1..rs.len()).any(|j| rs[i].intersects(&rs[j])))
    }

    fn positions(g: &Graph) -> Vec<(NodeId, f32, f32)> {
        g.nodes.iter().map(|n| (n.id, n.x, n.y)).collect()
    }

    #[test]
    fn tidy_leaves_nothing_overlapping() {
        for jitter in [0.0, 5.0, 11.0] {
            let mut g = messy(jitter);
            tidy(&mut g);
            assert!(!overlaps(&g));
        }
    }

    #[test]
    fn tidy_does_not_depend_on_where_nodes_started() {
        let (mut a, mut b) = (messy(0.0), messy(3.0));
        // Same order of items either way: the start stays below.
        tidy(&mut a);
        tidy(&mut b);
        assert_eq!(positions(&a), positions(&b));
        let again = positions(&a);
        tidy(&mut a);
        assert_eq!(again, positions(&a));
    }

    #[test]
    fn a_run_flows_left_to_right_and_values_sit_left_of_readers() {
        let mut g = messy(1.0);
        tidy(&mut g);
        for w in &g.wires {
            let (a, b) = (g.node(w.from.node).unwrap(), g.node(w.to.node).unwrap());
            assert!(
                a.x + rect(&g, a).w < b.x,
                "{} -> {} not left to right",
                a.kind,
                b.kind
            );
        }
    }

    #[test]
    fn place_new_keeps_fixed_nodes_and_avoids_overlap() {
        let mut g = messy(2.0);
        tidy(&mut g);
        let fixed: BTreeSet<NodeId> = g.nodes.iter().take(6).map(|n| n.id).collect();
        let before: Vec<_> = g.nodes.iter().take(6).map(|n| (n.x, n.y)).collect();
        for n in g.nodes.iter_mut().skip(6) {
            n.x = 0.0;
            n.y = 0.0;
        }
        let stray = add(&mut g, "print", [0.0, 0.0]);
        place_new(&mut g, &fixed);
        let after: Vec<_> = g.nodes.iter().take(6).map(|n| (n.x, n.y)).collect();
        assert_eq!(before, after);
        assert!(!overlaps(&g));
        assert!(g.node(stray).unwrap().y > 0.0);
    }

    #[test]
    fn a_refit_group_holds_its_nodes() {
        let mut g = Graph::default();
        let a = add(&mut g, "touched", [100.0, 100.0]);
        let b = add(&mut g, "print", [300.0, 140.0]);
        wire(&mut g, a, "", b, "");
        g.groups.push(Group {
            title: "Box".into(),
            x: 80.0,
            y: 80.0,
            w: 500.0,
            h: 200.0,
        });
        g.groups.push(Group {
            title: "Empty".into(),
            x: 900.0,
            y: 900.0,
            w: 50.0,
            h: 50.0,
        });
        tidy(&mut g);
        assert_eq!(g.nodes_within(0), vec![a, b]);
        assert_eq!((g.groups[1].x, g.groups[1].y), (900.0, 900.0));
    }
}
