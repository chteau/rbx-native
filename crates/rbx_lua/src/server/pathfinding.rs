//! `PathfindingService:ComputeAsync` in Local Run: our own navigation grid,
//! built for each request from the physics world's rays, and A* over it.
//!
//! What it matches on a live server (the Pathfinding golden): waypoints
//! lie on the floor (its top, not the agent's centre), one every
//! `WaypointSpacing` studs along each straight leg and at every corner,
//! labelled with the floor's material (or its `PathfindingModifier`'s
//! label); obstacles are kept about a stud away (a 3 stud doorway lets
//! even a wide agent through, as Roblox's does); a 3 stud ledge or a 6
//! stud gap needs a jump, whose waypoint is where the jump starts and is
//! labelled `Jump`; a `PathfindingLink` is a `Custom` waypoint at its first
//! attachment; `Costs` by material or label make a detour, `math.huge`
//! forbids; a goal inside a part goes to the part's top. Characters
//! (models with a Humanoid) are no obstacle, nor are parts that don't
//! collide.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use rbx_dom::{Ref, Variant};

use super::{Physics, QueryFilter};
use crate::ctx::Ctx;

/// Grid cells are one stud square.
const CELL: f64 = 1.0;
/// Walk up a step of at most this without jumping.
const STEP: f64 = 1.6;
/// The highest a jump reaches.
const JUMP_UP: f64 = 6.5;
/// The farthest a jump goes, in cells.
const JUMP_CELLS: i32 = 10;
/// Extra cost of a jump, so walking wins when it can.
const JUMP_COST: f64 = 4.0;
/// Room around the start and goal searched for a way.
const MARGIN: f64 = 24.0;
/// The most cells a side, past which the region is clipped.
const MAX_SIDE: i64 = 384;
/// Surfaces looked for in one column.
const LAYERS: usize = 4;
/// Floors steeper than this aren't walkable (normal's y).
const MIN_NORMAL_Y: f64 = 0.6;
/// Snap the start and goal to a floor at most this far up or down.
const SNAP: f64 = 10.0;

/// What `CreatePath`'s agent parameters ask for.
#[derive(Clone, Debug)]
pub struct PathRequest {
    pub start: [f64; 3],
    pub goal: [f64; 3],
    pub height: f64,
    pub can_jump: bool,
    pub spacing: f64,
    /// Material or label → cost per stud; infinite forbids.
    pub costs: HashMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub position: [f64; 3],
    /// `Walk`, `Jump` or `Custom`.
    pub action: &'static str,
    pub label: String,
}

/// A walkable floor point of a column.
#[derive(Clone, Debug)]
struct Floor {
    y: f64,
    label: String,
    /// Kept a stud away from walls and drops.
    open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Node {
    x: i64,
    z: i64,
    layer: usize,
}

/// How one step of the search was made.
#[derive(Clone, Debug, PartialEq)]
enum Move {
    Walk,
    Jump,
    Link(String, [f64; 3], [f64; 3]),
}

struct Grid {
    x0: i64,
    z0: i64,
    nx: i64,
    nz: i64,
    columns: Vec<Vec<Floor>>,
}

impl Grid {
    fn column(&self, x: i64, z: i64) -> Option<&Vec<Floor>> {
        if x < self.x0 || z < self.z0 || x >= self.x0 + self.nx || z >= self.z0 + self.nz {
            return None;
        }
        self.columns
            .get(((x - self.x0) * self.nz + (z - self.z0)) as usize)
    }

    fn floor(&self, node: Node) -> Option<&Floor> {
        self.column(node.x, node.z)?.get(node.layer)
    }

    /// The floor of the column at `(x, z)` within `STEP` of `y`.
    fn level(&self, x: i64, z: i64, y: f64) -> Option<usize> {
        self.column(x, z)?
            .iter()
            .position(|f| (f.y - y).abs() <= STEP)
    }

    fn center(&self, node: Node) -> [f64; 3] {
        let y = self.floor(node).map_or(0.0, |f| f.y);
        [node.x as f64 * CELL, y, node.z as f64 * CELL]
    }
}

/// The parts of every character (a model holding a Humanoid).
fn characters(ctx: &Ctx) -> HashSet<Ref> {
    let dom = ctx.dom();
    let db = ctx.database();
    let mut out = HashSet::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        if instance.class() == "Humanoid" {
            if let Some(model) = dom.parent(r) {
                let mut inner = vec![model];
                while let Some(m) = inner.pop() {
                    if let Some(i) = dom.get(m) {
                        if db.is_subclass_of(i.class(), "BasePart") {
                            out.insert(m);
                        }
                        inner.extend(i.children().iter().copied());
                    }
                }
            }
        }
        stack.extend(instance.children().iter().copied());
    }
    out
}

/// What a hit part's floor is called (its modifier's label, or its
/// material), and whether its modifier lets agents through it.
fn describe(ctx: &Ctx, part: Ref, terrain: Option<&'static str>) -> (String, bool) {
    if let Some(material) = terrain {
        return (material.to_owned(), false);
    }
    let dom = ctx.dom();
    let Some(instance) = dom.get(part) else {
        return ("Plastic".to_owned(), false);
    };
    for child in instance.children() {
        let Some(modifier) = dom.get(*child) else {
            continue;
        };
        if modifier.class() != "PathfindingModifier" {
            continue;
        }
        let props = modifier.properties();
        let pass = matches!(props.get("PassThrough"), Some(Variant::Bool(true)));
        let label = match props.get("Label") {
            Some(Variant::String(s)) if !s.is_empty() => s.clone(),
            _ => material_name(ctx, instance),
        };
        return (label, pass);
    }
    (material_name(ctx, instance), false)
}

fn material_name(ctx: &Ctx, instance: &rbx_dom::Instance) -> String {
    let value = match instance.properties().get("Material") {
        Some(Variant::Enum(v)) => *v,
        _ => 256,
    };
    ctx.database()
        .enum_name("Material", value)
        .unwrap_or("Plastic")
        .to_owned()
}

/// Rays down a column: the floors an agent of `height` fits on, top first.
fn column(
    physics: &dyn Physics,
    ctx: &Ctx,
    filter: &QueryFilter,
    x: f64,
    z: f64,
    top: f64,
    bottom: f64,
    height: f64,
) -> Vec<Floor> {
    let mut floors = Vec::new();
    let mut filter = filter.clone();
    let mut from = top;
    // The last surface above, which a floor needs `height` of room under.
    let mut ceiling = f64::INFINITY;
    for _ in 0..LAYERS * 2 {
        if from <= bottom {
            break;
        }
        let Some(hit) = physics.raycast([x, from, z], [0.0, bottom - from, 0.0], &filter) else {
            break;
        };
        let y = hit.position[1];
        let (label, pass) = describe(ctx, hit.part, hit.material);
        if pass {
            filter.parts.insert(hit.part);
            from = y - 0.01;
            continue;
        }
        if hit.normal[1] >= MIN_NORMAL_Y && ceiling - y >= height {
            floors.push(Floor {
                y,
                label,
                open: true,
            });
            if floors.len() >= LAYERS {
                break;
            }
        }
        // Below this part: the next ray starts under it.
        if hit.material.is_none() {
            filter.parts.insert(hit.part);
        }
        ceiling = bottom_of(physics, &filter, hit.part, x, y, z).unwrap_or(y);
        from = ceiling - 0.01;
        if hit.material.is_some() {
            break;
        }
    }
    floors
}

/// Where the part just hit ends below (a ray up from far under it).
fn bottom_of(
    physics: &dyn Physics,
    filter: &QueryFilter,
    part: Ref,
    x: f64,
    y: f64,
    z: f64,
) -> Option<f64> {
    let only = QueryFilter {
        parts: [part].into_iter().collect(),
        include: true,
        ..filter.clone()
    };
    let below = y - 2048.0;
    physics
        .raycast([x, below, z], [0.0, y - below, 0.0], &only)
        .map(|hit| hit.position[1])
}

fn build(physics: &dyn Physics, ctx: &Ctx, request: &PathRequest) -> Grid {
    let (s, g) = (request.start, request.goal);
    let span = ((s[0] - g[0]).powi(2) + (s[2] - g[2]).powi(2)).sqrt();
    let margin = MARGIN + span * 0.25;
    let lo = |a: f64, b: f64| ((a.min(b) - margin) / CELL).floor() as i64;
    let hi = |a: f64, b: f64| ((a.max(b) + margin) / CELL).ceil() as i64;
    let (mut x0, mut x1) = (lo(s[0], g[0]), hi(s[0], g[0]));
    let (mut z0, mut z1) = (lo(s[2], g[2]), hi(s[2], g[2]));
    for (a, b) in [(&mut x0, &mut x1), (&mut z0, &mut z1)] {
        if *b - *a > MAX_SIDE {
            let mid = (*a + *b) / 2;
            *a = mid - MAX_SIDE / 2;
            *b = mid + MAX_SIDE / 2;
        }
    }
    let top = s[1].max(g[1]) + 48.0;
    let bottom = s[1].min(g[1]) - 64.0;
    let filter = QueryFilter {
        parts: characters(ctx),
        include: false,
        respect_can_collide: true,
        collision_group: "Default".to_owned(),
        max_parts: 0,
        ignore_water: false,
    };
    let (nx, nz) = (x1 - x0 + 1, z1 - z0 + 1);
    let mut columns = Vec::with_capacity((nx * nz) as usize);
    for x in x0..=x1 {
        for z in z0..=z1 {
            columns.push(column(
                physics,
                ctx,
                &filter,
                x as f64 * CELL,
                z as f64 * CELL,
                top,
                bottom,
                request.height,
            ));
        }
    }
    let mut grid = Grid {
        x0,
        z0,
        nx,
        nz,
        columns,
    };
    erode(&mut grid);
    grid
}

/// A floor next to a wall or a drop (a neighbour with no floor within a
/// step) is closed: agents keep a stud away.
fn erode(grid: &mut Grid) {
    let mut closed = Vec::new();
    for x in grid.x0..grid.x0 + grid.nx {
        for z in grid.z0..grid.z0 + grid.nz {
            let Some(floors) = grid.column(x, z) else {
                continue;
            };
            for (layer, floor) in floors.iter().enumerate() {
                let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| {
                    grid.column(x + dx, z + dz).is_some()
                        && grid.level(x + dx, z + dz, floor.y).is_none()
                });
                if edge {
                    closed.push(Node { x, z, layer });
                }
            }
        }
    }
    for node in closed {
        let index = ((node.x - grid.x0) * grid.nz + (node.z - grid.z0)) as usize;
        grid.columns[index][node.layer].open = false;
    }
}

fn cost_of(request: &PathRequest, label: &str) -> f64 {
    request.costs.get(label).copied().unwrap_or(1.0).max(0.0)
}

/// The open floor nearest `point` (the column under it first, then
/// around it), within `SNAP` up or down.
fn snap(grid: &Grid, point: [f64; 3]) -> Option<Node> {
    let cx = (point[0] / CELL).round() as i64;
    let cz = (point[2] / CELL).round() as i64;
    let mut best: Option<(f64, Node)> = None;
    for r in 0..=3i64 {
        for x in cx - r..=cx + r {
            for z in cz - r..=cz + r {
                if (x - cx).abs().max((z - cz).abs()) != r {
                    continue;
                }
                let Some(floors) = grid.column(x, z) else {
                    continue;
                };
                for (layer, f) in floors.iter().enumerate() {
                    let dy = (f.y - point[1]).abs();
                    if !f.open || dy > SNAP {
                        continue;
                    }
                    let d = dy + (r as f64) * 2.0;
                    if best.as_ref().is_none_or(|(b, _)| d < *b) {
                        best = Some((d, Node { x, z, layer }));
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, n)| n)
}

#[derive(PartialEq)]
struct Open(f64, Node);
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}

/// The links as (from node, to node, label, from point, to point).
type Links = Vec<(Node, Node, String, [f64; 3], [f64; 3])>;

fn neighbours(
    grid: &Grid,
    request: &PathRequest,
    links: &Links,
    node: Node,
) -> Vec<(Node, f64, Move)> {
    let mut out = Vec::new();
    let Some(here) = grid.floor(node) else {
        return out;
    };
    for dx in -1..=1i64 {
        for dz in -1..=1i64 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let (x, z) = (node.x + dx, node.z + dz);
            let Some(layer) = grid.level(x, z, here.y) else {
                continue;
            };
            let next = Node { x, z, layer };
            let Some(floor) = grid.floor(next) else {
                continue;
            };
            // No cutting a corner past a wall.
            if dx != 0
                && dz != 0
                && (grid.level(node.x + dx, node.z, here.y).is_none()
                    || grid.level(node.x, node.z + dz, here.y).is_none())
            {
                continue;
            }
            if !floor.open {
                continue;
            }
            let cost = cost_of(request, &floor.label);
            if !cost.is_finite() {
                continue;
            }
            let step = ((dx * dx + dz * dz) as f64).sqrt() * CELL;
            out.push((next, step * cost, Move::Walk));
        }
    }
    if request.can_jump {
        for (dx, dz) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            jumps(grid, request, node, here, dx, dz, &mut out);
        }
    }
    for (from, to, label, a, b) in links {
        if *from == node {
            out.push((*to, distance(*a, *b), Move::Link(label.clone(), *a, *b)));
        }
    }
    out
}

/// Jumps in one direction: past a gap or a ledge (a cell with no floor at
/// this level) onto the first open floor within reach, nothing on the way
/// higher than the jump.
fn jumps(
    grid: &Grid,
    request: &PathRequest,
    node: Node,
    here: &Floor,
    dx: i64,
    dz: i64,
    out: &mut Vec<(Node, f64, Move)>,
) {
    // Jumps start from the last open floor before the edge.
    let next = grid
        .level(node.x + dx, node.z + dz, here.y)
        .and_then(|layer| {
            grid.floor(Node {
                x: node.x + dx,
                z: node.z + dz,
                layer,
            })
        });
    if next.is_some_and(|f| f.open) {
        return;
    }
    let mut gap = false;
    for k in 1..=JUMP_CELLS as i64 {
        let (x, z) = (node.x + dx * k, node.z + dz * k);
        let Some(floors) = grid.column(x, z) else {
            return;
        };
        let wall = floors
            .iter()
            .any(|f| f.y > here.y + JUMP_UP + 0.5 && f.y < here.y + JUMP_UP + request.height);
        if wall {
            return;
        }
        gap |= grid.level(x, z, here.y).is_none();
        if !gap {
            continue;
        }
        for (layer, f) in floors.iter().enumerate() {
            let dy = f.y - here.y;
            if !f.open || dy > JUMP_UP || dy < -JUMP_UP * 3.0 {
                continue;
            }
            let cost = cost_of(request, &f.label);
            if !cost.is_finite() {
                continue;
            }
            let reach = ((dx * dx + dz * dz) as f64).sqrt() * k as f64 * CELL;
            out.push((Node { x, z, layer }, reach * cost + JUMP_COST, Move::Jump));
            return;
        }
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn links(physics_links: &[([f64; 3], [f64; 3], String, bool)], grid: &Grid) -> Links {
    let mut out = Vec::new();
    for (a, b, label, both) in physics_links {
        let (Some(na), Some(nb)) = (snap(grid, *a), snap(grid, *b)) else {
            continue;
        };
        out.push((na, nb, label.clone(), *a, *b));
        if *both {
            out.push((nb, na, label.clone(), *b, *a));
        }
    }
    out
}

/// Every `PathfindingLink`: its attachments' world positions, label and
/// whether it goes both ways.
fn find_links(ctx: &Ctx) -> Vec<([f64; 3], [f64; 3], String, bool)> {
    let dom = ctx.dom();
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        stack.extend(instance.children().iter().copied());
        if instance.class() != "PathfindingLink" {
            continue;
        }
        let props = instance.properties();
        let attachment = |name: &str| match props.get(name) {
            Some(Variant::Ref(a)) => crate::property::pseudo::attachment_world_position(ctx, *a),
            _ => None,
        };
        let (Some(a), Some(b)) = (attachment("Attachment0"), attachment("Attachment1")) else {
            continue;
        };
        let label = match props.get("Label") {
            Some(Variant::String(s)) => s.clone(),
            _ => String::new(),
        };
        let both = !matches!(props.get("IsBidirectional"), Some(Variant::Bool(false)));
        out.push((a, b, label, both));
    }
    out
}

/// A* from `start` to `goal`: the nodes and how each was reached.
fn search(
    grid: &Grid,
    request: &PathRequest,
    links: &Links,
    start: Node,
    goal: Node,
) -> Option<Vec<(Node, Move)>> {
    let mut best: HashMap<Node, f64> = HashMap::new();
    let mut came: HashMap<Node, (Node, Move)> = HashMap::new();
    let mut open = BinaryHeap::new();
    let target = grid.center(goal);
    let h = |n: Node| {
        let c = grid.center(n);
        ((c[0] - target[0]).powi(2) + (c[2] - target[2]).powi(2)).sqrt()
    };
    best.insert(start, 0.0);
    open.push(Open(h(start), start));
    let mut expanded = 0usize;
    while let Some(Open(_, node)) = open.pop() {
        if node == goal {
            let mut path = vec![(goal, Move::Walk)];
            let mut at = goal;
            while let Some((prev, how)) = came.get(&at) {
                path.last_mut().expect("not empty").1 = how.clone();
                path.push((*prev, Move::Walk));
                at = *prev;
            }
            path.reverse();
            // Each entry's move is how the next one is reached.
            let moves: Vec<Move> = path.iter().map(|(_, m)| m.clone()).collect();
            let mut out = Vec::with_capacity(path.len());
            for (i, (n, _)) in path.iter().enumerate() {
                let how = if i + 1 < path.len() {
                    moves[i + 1].clone()
                } else {
                    Move::Walk
                };
                out.push((*n, how));
            }
            return Some(out);
        }
        expanded += 1;
        if expanded > 400_000 {
            return None;
        }
        let g = best[&node];
        for (next, cost, how) in neighbours(grid, request, links, node) {
            let candidate = g + cost;
            if best.get(&next).is_none_or(|b| candidate < *b) {
                best.insert(next, candidate);
                came.insert(next, (node, how));
                open.push(Open(candidate + h(next), next));
            }
        }
    }
    None
}

/// Whether the straight walk between two nodes at the same level stays on
/// open floor no costlier than `limit` and never steps more than a step.
fn walkable_line(grid: &Grid, request: &PathRequest, a: Node, b: Node, limit: f64) -> bool {
    let pa = grid.center(a);
    let pb = grid.center(b);
    let length = ((pb[0] - pa[0]).powi(2) + (pb[2] - pa[2]).powi(2)).sqrt();
    let samples = (length / (CELL * 0.25)).ceil().max(1.0) as i64;
    let mut y = pa[1];
    for i in 0..=samples {
        let t = i as f64 / samples as f64;
        let x = pa[0] + (pb[0] - pa[0]) * t;
        let z = pa[2] + (pb[2] - pa[2]) * t;
        // Every cell the line's footprint touches, so it can't graze a wall.
        for (ox, oz) in [
            (0.0, 0.0),
            (0.3, 0.3),
            (-0.3, 0.3),
            (0.3, -0.3),
            (-0.3, -0.3),
        ] {
            let cx = ((x + ox) / CELL).round() as i64;
            let cz = ((z + oz) / CELL).round() as i64;
            let Some(layer) = grid.level(cx, cz, y) else {
                return false;
            };
            let Some(floor) = grid.floor(Node {
                x: cx,
                z: cz,
                layer,
            }) else {
                return false;
            };
            if !floor.open || cost_of(request, &floor.label) > limit {
                return false;
            }
            if ox == 0.0 {
                y = floor.y;
            }
        }
    }
    true
}

/// The path pulled tight: corners only, and every node a jump or a link
/// starts from.
fn smooth(grid: &Grid, request: &PathRequest, path: &[(Node, Move)]) -> Vec<(Node, Move)> {
    let cost = |n: Node| grid.floor(n).map_or(1.0, |f| cost_of(request, &f.label));
    let mut out = vec![(path[0].0, Move::Walk)];
    let mut i = 0;
    while i + 1 < path.len() {
        if path[i].1 != Move::Walk {
            out.last_mut().expect("not empty").1 = path[i].1.clone();
            i += 1;
            out.push((path[i].0, Move::Walk));
            continue;
        }
        let mut j = i + 1;
        let mut limit = cost(path[i].0);
        for k in i + 1..path.len() {
            limit = limit.max(cost(path[k].0));
            if walkable_line(grid, request, path[i].0, path[k].0, limit) {
                j = k;
            }
            if path[k].1 != Move::Walk {
                break;
            }
        }
        out.push((path[j].0, Move::Walk));
        i = j;
    }
    out
}

fn waypoints(
    grid: &Grid,
    request: &PathRequest,
    path: &[(Node, Move)],
    start: [f64; 3],
    goal: [f64; 3],
) -> Vec<Waypoint> {
    let label = |n: Node| grid.floor(n).map_or_else(String::new, |f| f.label.clone());
    let mut points: Vec<([f64; 3], Move, String)> = Vec::new();
    let last = path.len() - 1;
    for (i, (node, how)) in path.iter().enumerate() {
        let mut p = grid.center(*node);
        if i == 0 {
            p = [start[0], p[1], start[2]];
        } else if i == last {
            p = [goal[0], p[1], goal[2]];
        }
        if let Move::Link(_, a, _) = how {
            p = *a;
        }
        if i > 0 {
            if let Move::Link(_, _, b) = &path[i - 1].1 {
                p = *b;
            }
        }
        points.push((p, how.clone(), label(*node)));
    }
    let spacing = if request.spacing.is_finite() && request.spacing > 0.0 {
        request.spacing
    } else {
        f64::INFINITY
    };
    let mut out = Vec::new();
    for (i, (p, how, floor)) in points.iter().enumerate() {
        // A waypoint's action is how it's reached.
        let reached = if i == 0 {
            &Move::Walk
        } else {
            &points[i - 1].1
        };
        let (action, label) = match reached {
            Move::Walk => ("Walk", floor.clone()),
            Move::Jump => ("Jump", "Jump".to_owned()),
            Move::Link(name, ..) => ("Custom", name.clone()),
        };
        out.push(Waypoint {
            position: *p,
            action,
            label,
        });
        let Some((q, _, _)) = points.get(i + 1) else {
            break;
        };
        if *how != Move::Walk {
            continue;
        }
        let length = ((q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
        let mut d = spacing;
        while d < length - 1e-6 {
            let t = d / length;
            let at = [
                p[0] + (q[0] - p[0]) * t,
                p[1] + (q[1] - p[1]) * t,
                p[2] + (q[2] - p[2]) * t,
            ];
            let cx = (at[0] / CELL).round() as i64;
            let cz = (at[2] / CELL).round() as i64;
            let here = grid.level(cx, cz, at[1]).and_then(|l| {
                grid.floor(Node {
                    x: cx,
                    z: cz,
                    layer: l,
                })
            });
            out.push(Waypoint {
                position: [at[0], here.map_or(at[1], |f| f.y), at[2]],
                action: "Walk",
                label: here.map_or_else(|| floor.clone(), |f| f.label.clone()),
            });
            d += spacing;
        }
    }
    out
}

/// A path for `request`, `None` when there's no way (`NoPath`).
pub fn compute(physics: &dyn Physics, ctx: &Ctx, request: &PathRequest) -> Option<Vec<Waypoint>> {
    let grid = build(physics, ctx, request);
    let start = snap(&grid, request.start)?;
    let goal = snap(&grid, request.goal)?;
    let links = links(&find_links(ctx), &grid);
    let path = search(&grid, request, &links, start, goal)?;
    let path = smooth(&grid, request, &path);
    Some(waypoints(
        &grid,
        request,
        &path,
        request.start,
        request.goal,
    ))
}

/// `Path:CheckOcclusionAsync(from)`: the first waypoint from `from` (1-based)
/// that something now stands in, or on the way to which it does; -1 when
/// the way is clear.
pub fn occlusion(
    physics: &dyn Physics,
    ctx: &Ctx,
    waypoints: &[[f64; 3]],
    from: usize,
    height: f64,
) -> i64 {
    let filter = QueryFilter {
        parts: characters(ctx),
        include: false,
        respect_can_collide: true,
        collision_group: "Default".to_owned(),
        max_parts: 0,
        ignore_water: true,
    };
    let start = from.max(1);
    for i in start..=waypoints.len() {
        let p = waypoints[i - 1];
        let up = physics.raycast([p[0], p[1] + 0.1, p[2]], [0.0, height - 0.1, 0.0], &filter);
        let mut blocked = up.is_some();
        if !blocked && i > 1 {
            let q = waypoints[i - 2];
            let a = [q[0], q[1] + 1.0, q[2]];
            let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
            blocked = physics.raycast(a, d, &filter).is_some();
        }
        if blocked {
            return i as i64;
        }
    }
    -1
}
