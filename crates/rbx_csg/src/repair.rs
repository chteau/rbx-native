//! Turning the BSP's polygon soup into a mesh fit to draw, and deciding
//! whether it is: snapping the near-coincident vertices Roblox's own
//! floating-point noise leaves apart, closing T-junctions, dropping
//! disconnected debris, and the watertightness check [`super::Solid::finish`]
//! gates the result on.

use std::collections::HashMap;

use glam::DVec3;

use super::bsp::Polygon;

/// Groups `polygons` by shared-edge adjacency (union-find), then keeps: the
/// largest group with positive signed volume (the main shell), every other
/// positive group that lies inside `bake` (the extent Studio's own bake gave
/// the union, centred on its frame, `None` keeps them all), and *every*
/// group with non-positive volume, however small.
///
/// A union of separate parts is separate pieces in Studio too (a keyboard's
/// keys, a row of bricks), so a piece is only debris when it lies where
/// Studio's own result has nothing: a sliver the BSP left outside the real
/// shape. A negative-volume group is a surface facing inward, a fully
/// enclosed cavity, like a negation sitting entirely inside the additive
/// solid, which is a topologically required complement to the shell it
/// hollows out; dropping it would silently undo the carve it represents.
/// `is_watertight` still holds afterward: every discarded group is its own
/// individually closed piece, so removing it only removes matched edge
/// pairs, never leaves one side dangling.
pub fn discard_disconnected_debris(polygons: Vec<Polygon>, bake: Option<DVec3>) -> Vec<Polygon> {
    let roots = component_roots(&polygons);
    // A negative-volume component is an inward-facing surface, a fully
    // enclosed cavity (the classic case: a negation entirely inside the
    // additive solid, like a bubble) rather than a piece of the outer shell.
    // It is topologically required, however small: dropping it would silently
    // undo the carve it represents. Only a same-signed (positive) component
    // competes to be kept; every negative one always survives.
    let mut volume: HashMap<usize, f64> = HashMap::new();
    let mut positive_size: HashMap<usize, usize> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let root = roots[i];
        let first = polygon.vertices[0];
        let v: f64 = polygon.vertices[1..]
            .windows(2)
            .map(|pair| first.dot(pair[0].cross(pair[1])))
            .sum();
        *volume.entry(root).or_insert(0.0) += v / 6.0;
    }
    let mut extent: HashMap<usize, (DVec3, DVec3)> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let root = roots[i];
        if volume[&root] > 0.0 {
            *positive_size.entry(root).or_insert(0) += 1;
        }
        let (min, max) = extent
            .entry(root)
            .or_insert((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)));
        for &v in &polygon.vertices {
            *min = min.min(v);
            *max = max.max(v);
        }
    }
    let largest_positive = positive_size
        .iter()
        .max_by_key(|&(_, &count)| count)
        .map(|(&root, _)| root);
    // Studio's bake is centred on the union's own frame, `bake` across.
    let fits = |root: usize| {
        bake.is_none_or(|bake| {
            let half = bake / 2.0;
            let pad = half * BAKE_TOLERANCE + DVec3::splat(BAKE_PAD);
            let (min, max) = extent[&root];
            (min.cmpge(-half - pad) & max.cmple(half + pad)).all()
        })
    };
    let keep: std::collections::HashSet<usize> = volume
        .keys()
        .copied()
        .filter(|&root| volume[&root] <= 0.0 || Some(root) == largest_positive || fits(root))
        .collect();
    polygons
        .into_iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(&roots[*i]))
        .map(|(_, polygon)| polygon)
        .collect()
}

/// The connected piece each polygon belongs to, by shared-edge adjacency
/// (union-find): one representative polygon index per piece.
pub fn component_roots(polygons: &[Polygon]) -> Vec<usize> {
    let key = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let mut parent: Vec<usize> = (0..polygons.len()).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    let mut edge_owner: HashMap<DirectedEdge, usize> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let n = polygon.vertices.len();
        for e in 0..n {
            let a = key(polygon.vertices[e]);
            let b = key(polygon.vertices[(e + 1) % n]);
            let edge = if a <= b { (a, b) } else { (b, a) };
            match edge_owner.get(&edge) {
                Some(&owner) => {
                    let (ra, rb) = (find(&mut parent, owner), find(&mut parent, i));
                    if ra != rb {
                        parent[ra] = rb;
                    }
                }
                None => {
                    edge_owner.insert(edge, i);
                }
            }
        }
    }
    (0..polygons.len()).map(|i| find(&mut parent, i)).collect()
}

/// How far, relative to the bake's own half-extent and then absolutely in
/// studs, a piece may poke out of it and still be kept: Studio rounds its own
/// bake a little, and leaves are placed with float noise of their own.
const BAKE_TOLERANCE: f64 = 0.02;
const BAKE_PAD: f64 = 0.01;

/// Snap tolerance for the edge-adjacency check below: coarser than any
/// legitimate vertex spacing this codebase's shapes produce, fine enough not
/// to merge genuinely distinct nearby edges.
pub const WELD_EPSILON: f64 = 1e-4;

/// A vertex position snapped to the `WELD_EPSILON` grid, used to recognize
/// "the same point" (or edge) across independently-computed fragments.
pub type GridPoint = (i64, i64, i64);
pub type DirectedEdge = (GridPoint, GridPoint);

fn grid(v: DVec3) -> GridPoint {
    (
        (v.x / WELD_EPSILON).round() as i64,
        (v.y / WELD_EPSILON).round() as i64,
        (v.z / WELD_EPSILON).round() as i64,
    )
}

/// Moves every vertex onto the first one seen within [`WELD_EPSILON`] of
/// it, then drops what that collapses: a repeated corner, a spike (a corner
/// the ring doubles straight back from), and a polygon left with fewer than
/// three corners.
///
/// Roblox's own coordinates carry about 1e-5 of float noise, so two faces
/// meant to meet can sit 2–4e-5 apart, just past the BSP's split
/// tolerance, and the boolean leaves slivers that thin between them. They
/// have no visible area, but their edges read as doubled or zero-length to
/// [`is_watertight`]; on real places that was most of what failed it.
/// Neighbouring cells are searched as well as a vertex's own, so two noisy
/// copies of one corner that straddle a cell boundary still meet.
pub fn clean(polygons: Vec<Polygon>) -> Vec<Polygon> {
    let mut cells: HashMap<GridPoint, Vec<DVec3>> = HashMap::new();
    let mut snap = |v: DVec3| {
        let (x, y, z) = grid(v);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let near = cells.get(&(x + dx, y + dy, z + dz)).and_then(|seen| {
                        seen.iter()
                            .find(|&&p| (p - v).length_squared() <= WELD_EPSILON * WELD_EPSILON)
                    });
                    if let Some(&p) = near {
                        return p;
                    }
                }
            }
        }
        cells.entry((x, y, z)).or_default().push(v);
        v
    };
    polygons
        .into_iter()
        .filter_map(|polygon| {
            let mut ring: Vec<DVec3> = polygon.vertices.into_iter().map(&mut snap).collect();
            loop {
                let before = ring.len();
                ring.dedup();
                while ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                let n = ring.len();
                if n >= 3 {
                    if let Some(spike) =
                        (0..n).find(|&i| ring[(i + n - 1) % n] == ring[(i + 1) % n])
                    {
                        ring.remove(spike);
                    }
                }
                if ring.len() == before {
                    break;
                }
            }
            (ring.len() >= 3).then_some(Polygon {
                vertices: ring,
                plane: polygon.plane,
            })
        })
        .collect()
}

/// A fragment is finalized as soon as `clip_polygons` confirms it lies in
/// front of just one of the other solid's planes, correct, since that alone
/// proves it is outside a convex cutter, but it means a neighboring fragment
/// that needed more plane tests can have their shared boundary edge
/// subdivided on its side and not on this one. Splits every polygon edge at
/// any other polygon's vertex that lies exactly on it (using that vertex's
/// own value, not a recomputed one, so the two sides end up bit-identical)
/// to turn that T-junction back into a matching pair of edges before
/// [`is_watertight`] ever runs.
pub fn weld_t_junctions(polygons: Vec<Polygon>) -> Vec<Polygon> {
    const T_MARGIN: f64 = 1e-7;
    let quantize = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let mut canonical: HashMap<GridPoint, DVec3> = HashMap::new();
    for polygon in &polygons {
        for &v in &polygon.vertices {
            canonical.entry(quantize(v)).or_insert(v);
        }
    }
    // An O(edges * vertices) pass only stays cheap for a modest vertex
    // count; past this, skip welding and let `is_watertight` (or the
    // triangle cap, further up the call chain) catch a genuine problem.
    if canonical.len() > 20_000 {
        return polygons;
    }
    let points: Vec<DVec3> = canonical.into_values().collect();
    polygons
        .into_iter()
        .map(|polygon| {
            let n = polygon.vertices.len();
            let mut ring = Vec::with_capacity(n + 4);
            for i in 0..n {
                let a = polygon.vertices[i];
                let b = polygon.vertices[(i + 1) % n];
                ring.push(a);
                let edge = b - a;
                let length_sq = edge.length_squared();
                if length_sq < WELD_EPSILON * WELD_EPSILON {
                    continue;
                }
                let mut inserts: Vec<(f64, DVec3)> = points
                    .iter()
                    .filter_map(|&p| {
                        let t = (p - a).dot(edge) / length_sq;
                        if !(T_MARGIN..=1.0 - T_MARGIN).contains(&t) {
                            return None;
                        }
                        let on_line = a + edge * t;
                        ((p - on_line).length_squared() < WELD_EPSILON * WELD_EPSILON)
                            .then_some((t, p))
                    })
                    .collect();
                inserts.sort_by(|x, y| x.0.total_cmp(&y.0));
                ring.extend(inserts.into_iter().map(|(_, p)| p));
            }
            Polygon {
                vertices: ring,
                plane: polygon.plane,
            }
        })
        .collect()
}

/// A lone stray edge stays invisible at render distance, the risk this
/// guards against is a shattered, hole-ridden mesh, not a single hairline
/// sliver. Anything past this fraction of the mesh's own edges reads as
/// broken rather than merely imperfect (the additive-only fallback is safer
/// past that point); below it, [`weld_t_junctions`] has already done what it
/// can and the rest is the BSP split's inherent floating-point residue.
const MAX_LEAK_FRACTION: f64 = 0.02;

/// Every directed polygon edge in a correctly closed, consistently-wound
/// solid is matched by as many of its reverse, the invariant a leak (an
/// open edge) breaks. As many, not exactly one: two parts a builder set
/// touching only along an edge (a door frame's jambs and lintel) meet in a
/// legitimately non-manifold edge that four faces share. Called once, by
/// [`evaluate`], on the finished result.
pub fn is_watertight(polygons: &[Polygon]) -> bool {
    let key = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let mut directed: HashMap<DirectedEdge, u32> = HashMap::new();
    for polygon in polygons {
        let n = polygon.vertices.len();
        for i in 0..n {
            let a = key(polygon.vertices[i]);
            let b = key(polygon.vertices[(i + 1) % n]);
            *directed.entry((a, b)).or_insert(0) += 1;
        }
    }
    let bad: Vec<DirectedEdge> = directed
        .iter()
        .filter(|(&(a, b), &count)| a == b || directed.get(&(b, a)).copied() != Some(count))
        .map(|(&edge, _)| edge)
        .collect();
    (bad.len() as f64) <= (directed.len() as f64) * MAX_LEAK_FRACTION || only_slivers(&bad)
}

/// How wide, in studs, a gap may be and still count as a sliver: a hundredth
/// of the smallest part Roblox lets a builder make.
const SLIVER_GAP: f64 = 1e-3;
const MAX_SLIVER_EDGES: usize = 512;

/// Whether every open edge in `bad` borders a gap no wider than
/// [`SLIVER_GAP`]: either it is that short itself, or another open edge runs
/// back alongside it within that distance. Such a gap, a cylinder's facet
/// corner landing a few ten-thousandths off a box's edge, has no area
/// anyone could see, however many of a small mesh's edges it touches.
pub fn only_slivers(bad: &[DirectedEdge]) -> bool {
    // Pairwise: past this many open edges the mesh is broken anyway, and
    // the check would cost more than the boolean.
    if bad.len() > MAX_SLIVER_EDGES {
        return false;
    }
    let point = |(x, y, z): GridPoint| DVec3::new(x as f64, y as f64, z as f64) * WELD_EPSILON;
    let near = |p: GridPoint, q: GridPoint| point(p).distance(point(q)) <= SLIVER_GAP;
    bad.iter().all(|&(a, b)| {
        near(a, b)
            || bad
                .iter()
                .any(|&(c, d)| (c, d) != (a, b) && near(c, b) && near(d, a))
    })
}
