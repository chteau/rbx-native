//! Where the require graph's boxes go: one column per layer, a script that
//! nothing requires on the left and each module one column right of the
//! furthest script requiring it, so edges run left to right. A cycle's
//! modules share one column (the layering would otherwise never settle),
//! and the edges between boxes of one column are drawn as arcs out to the
//! right. An unknown require is a ghost box in the column after its
//! script's. Within a column, boxes are ordered by the mean row of what
//! requires them, which uncrosses most edges.

use std::collections::HashMap;

use rbx_audit::{Edge, RequireGraph, Target};
use rbx_dom::Ref;

pub(super) const NODE_WIDTH: f32 = 220.;
pub(super) const NODE_HEIGHT: f32 = 48.;
pub(super) const GHOST_HEIGHT: f32 = 32.;
const COLUMN_GAP: f32 = 120.;
/// How far an arc between boxes of one column bows out to the right.
const ARC: f32 = 56.;
const ROW_GAP: f32 = 16.;
pub(super) const MARGIN: f32 = 24.;

/// One box: a node by its index in `RequireGraph::nodes`, or the unknown
/// require of an edge by its index in `RequireGraph::edges`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    Node(usize),
    Ghost(usize),
}

#[derive(Default)]
pub(super) struct Layout {
    /// Each box and its top-left corner.
    pub(super) items: Vec<(Item, [f32; 2])>,
    pub(super) nodes: HashMap<Ref, [f32; 2]>,
    pub(super) ghosts: HashMap<usize, [f32; 2]>,
    pub(super) size: [f32; 2],
}

impl Layout {
    pub(super) fn of(graph: &RequireGraph) -> Layout {
        let index: HashMap<Ref, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.script, i))
            .collect();
        let links: Vec<(usize, usize)> = graph
            .edges
            .iter()
            .filter(|e| !e.cycle)
            .filter_map(|e| match e.target {
                Target::Module(to) => Some((*index.get(&e.from)?, *index.get(&to)?)),
                Target::Unknown(_) => None,
            })
            .collect();
        let groups: Vec<Vec<usize>> = graph
            .cycles
            .iter()
            .map(|cycle| cycle.iter().filter_map(|r| index.get(r).copied()).collect())
            .collect();
        let mut layer = vec![0_usize; graph.nodes.len()];
        // Longest path over the graph with each cycle as one box settles
        // within one pass per node.
        for _ in 0..=graph.nodes.len() {
            let mut moved = false;
            for &(from, to) in &links {
                if layer[to] <= layer[from] {
                    layer[to] = layer[from] + 1;
                    moved = true;
                }
            }
            for group in &groups {
                let top = group.iter().map(|&i| layer[i]).max().unwrap_or(0);
                for &i in group {
                    moved |= layer[i] != top;
                    layer[i] = top;
                }
            }
            if !moved {
                break;
            }
        }

        let mut columns: Vec<Vec<(Item, f32)>> = Vec::new();
        let mut put = |column: usize, item: Item, height: f32| {
            if columns.len() <= column {
                columns.resize_with(column + 1, Vec::new);
            }
            columns[column].push((item, height));
        };
        for (i, &column) in layer.iter().enumerate() {
            put(column, Item::Node(i), NODE_HEIGHT);
        }
        for (i, edge) in graph.edges.iter().enumerate() {
            if let (Target::Unknown(_), Some(&from)) = (&edge.target, index.get(&edge.from)) {
                put(layer[from] + 1, Item::Ghost(i), GHOST_HEIGHT);
            }
        }

        let mut rows: HashMap<usize, f32> = HashMap::new();
        for column in &mut columns {
            let mean = |item: &Item| -> f32 {
                let Item::Node(to) = *item else {
                    return f32::MAX;
                };
                let above: Vec<f32> = links
                    .iter()
                    .filter(|link| link.1 == to)
                    .filter_map(|link| rows.get(&link.0).copied())
                    .collect();
                if above.is_empty() {
                    f32::MAX - 1.
                } else {
                    above.iter().sum::<f32>() / above.len() as f32
                }
            };
            column.sort_by(|a, b| mean(&a.0).total_cmp(&mean(&b.0)));
            for (row, (item, _)) in column.iter().enumerate() {
                if let Item::Node(i) = item {
                    rows.insert(*i, row as f32);
                }
            }
        }

        let mut layout = Layout::default();
        let mut bottom: f32 = 0.;
        for (column, items) in columns.iter().enumerate() {
            let x = MARGIN + column as f32 * (NODE_WIDTH + COLUMN_GAP);
            let mut y = MARGIN;
            for &(item, height) in items {
                let at = [x, y];
                match item {
                    Item::Node(i) => layout.nodes.insert(graph.nodes[i].script, at),
                    Item::Ghost(i) => layout.ghosts.insert(i, at),
                };
                layout.items.push((item, at));
                y += height + ROW_GAP;
            }
            bottom = bottom.max(y);
        }
        layout.size = [
            MARGIN * 2. + columns.len() as f32 * (NODE_WIDTH + COLUMN_GAP) - COLUMN_GAP,
            bottom - ROW_GAP + MARGIN,
        ];
        layout
    }
}

/// An edge as a cubic Bézier, `[start, control, control, end]`, and the
/// direction its arrow points along x at the end (`1.` right, `-1.` left).
pub(super) fn curve(edge: &Edge, index: usize, layout: &Layout) -> Option<([[f32; 2]; 4], f32)> {
    let from = layout.nodes.get(&edge.from)?;
    let right = from[0] + NODE_WIDTH;
    let (end, arc) = match edge.target {
        Target::Module(to) => {
            let to = layout.nodes.get(&to)?;
            if to[0] == from[0] {
                ([right, to[1] + NODE_HEIGHT / 2.], true)
            } else {
                ([to[0], to[1] + NODE_HEIGHT / 2.], false)
            }
        }
        Target::Unknown(_) => {
            let ghost = layout.ghosts.get(&index)?;
            ([ghost[0], ghost[1] + GHOST_HEIGHT / 2.], false)
        }
    };
    let start = [right, from[1] + NODE_HEIGHT / 2.];
    if arc {
        // A module requiring itself still needs a loop to show.
        let bow = ARC + (end[1] - start[1]).abs().min(NODE_HEIGHT) * 0.25;
        let end = if end == start {
            [right, from[1] + 6.]
        } else {
            end
        };
        let points = [start, [right + bow, start[1]], [right + bow, end[1]], end];
        return Some((points, -1.));
    }
    let reach = ((end[0] - start[0]).abs() * 0.5).max(60.);
    let points = [
        start,
        [start[0] + reach, start[1]],
        [end[0] - reach, end[1]],
        end,
    ];
    Some((points, 1.))
}

pub(super) fn at(points: &[[f32; 2]; 4], t: f32) -> [f32; 2] {
    let u = 1. - t;
    let weights = [u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t];
    let mut out = [0.; 2];
    for (point, weight) in points.iter().zip(weights) {
        out[0] += point[0] * weight;
        out[1] += point[1] * weight;
    }
    out
}

/// Where an edge's line badge sits: in the gap just before the box it
/// lands on, so a long edge's badge never covers a box it passes, or at
/// the bow of an arc.
pub(super) fn badge(points: &[[f32; 2]; 4], direction: f32) -> [f32; 2] {
    if direction < 0. {
        return at(points, 0.5);
    }
    let goal = points[3][0] - COLUMN_GAP / 2.;
    let (mut low, mut high) = (0., 1.);
    for _ in 0..24 {
        let mid = (low + high) / 2.;
        if at(points, mid)[0] < goal {
            low = mid;
        } else {
            high = mid;
        }
    }
    at(points, low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::{Variant, WeakDom};

    #[test]
    fn modules_sit_right_of_what_requires_them_and_ghosts_follow_their_script() {
        let mut dom = WeakDom::new();
        let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
        let script = |dom: &mut WeakDom, class: &str, name: &str, source: &str| {
            let r = dom.new_instance(class, name, Some(storage));
            dom.set_property(r, "Source", Variant::String(source.into()))
                .unwrap();
            r
        };
        let leaf = script(&mut dom, "ModuleScript", "Leaf", "return {}");
        let mid = script(
            &mut dom,
            "ModuleScript",
            "Mid",
            "return require(script.Parent.Leaf)",
        );
        let top = script(
            &mut dom,
            "Script",
            "Top",
            "require(script.Parent.Mid)\nrequire(script.Parent.Leaf)\nrequire(42)",
        );
        let graph = rbx_audit::require_graph(&dom);
        let layout = Layout::of(&graph);
        let x = |r: Ref| layout.nodes[&r][0];
        assert!(x(top) < x(mid) && x(mid) < x(leaf));
        let ghost = graph
            .edges
            .iter()
            .position(|e| matches!(e.target, Target::Unknown(_)))
            .unwrap();
        assert_eq!(layout.ghosts[&ghost][0], x(mid));
        assert!(layout.size[0] >= x(leaf) + NODE_WIDTH);
    }

    #[test]
    fn a_cycle_shares_one_column_and_its_edges_bow_right() {
        let mut dom = WeakDom::new();
        let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
        let mut module = |name: &str, next: &str| {
            let r = dom.new_instance("ModuleScript", name, Some(storage));
            let source = format!("return require(script.Parent.{next})");
            dom.set_property(r, "Source", Variant::String(source))
                .unwrap();
            r
        };
        let a = module("A", "B");
        let b = module("B", "A");
        let graph = rbx_audit::require_graph(&dom);
        let layout = Layout::of(&graph);
        assert_eq!(layout.nodes[&a][0], layout.nodes[&b][0]);
        let (points, direction) = curve(&graph.edges[0], 0, &layout).unwrap();
        assert_eq!(direction, -1.);
        assert!(points[1][0] > layout.nodes[&a][0] + NODE_WIDTH);
    }
}
