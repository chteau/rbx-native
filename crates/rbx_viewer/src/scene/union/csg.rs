//! Boolean geometry for legacy unions: union and subtraction over closed
//! triangle meshes, a BSP-tree CSG ported from Evan Wallace's csg.js (MIT).
//! Every polygon carries its own plane; a boolean is "clip each solid's
//! polygons against the other's tree, then merge". Entry point: [`evaluate`].
//!
//! Everything is `f64`: the splits chain through dozens of operations, and
//! `f32` classification drifts far enough to open seams along coplanar faces.

mod bsp;

use std::collections::HashMap;

use glam::{DVec3, Mat4, Vec3};
use rbx_dom::Variant;

use super::tree::Node;
use crate::scene::ShapeKind;
use crate::shapes::{self, MeshData};
use bsp::{BspNode, Plane, Polygon};

/// A result that outgrows this many polygons after any single step is
/// abandoned: the fallback box is better than a load stalled on one asset.
const MAX_POLYGONS: usize = 200_000;
/// Same idea for the operation tree itself: nobody hand-builds a union this
/// wide, so anything past it is a malformed asset rather than a rock.
const MAX_LEAVES: usize = 512;

/// Why a union did not turn into a mesh; the caller falls back to boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    TooComplex,
    /// Every polygon carved away (or none to begin with).
    Empty,
    /// The result still has too many open edges after welding: a BSP split
    /// against a rotated plane can leave a T-junction (`weld_t_junctions`
    /// closes most of these) or, rarely, a genuine gap. `MAX_LEAK_FRACTION`
    /// decides how much residue is a harmless sliver versus this. Caught
    /// here so a badly leaky result never reaches the renderer.
    Leaky,
    /// The signed volume is non-positive, or exceeds the sum of every
    /// additive leaf's own volume — a global (or large partial) winding
    /// flip, or a boolean that grew instead of shrank. See [`evaluate`].
    Inverted,
}

/// A closed polygon soup, the unit of every boolean here.
#[derive(Debug, Clone, Default)]
pub(super) struct Solid {
    polygons: Vec<Polygon>,
}

/// Re-fuses the two triangles of each flat quad face (every shape generator
/// in `crate::shapes` emits a face as two triangles sharing a diagonal, back
/// to back — see `MeshData::push_flat_quad`) back into one 4-vertex polygon.
///
/// The diagonal is purely a rasterizer convenience; feeding the BSP tree a
/// triangulated face instead of the whole flat face gives every cutting plane
/// two independent fragmentation histories for the same physical surface, and
/// a many-cut boolean (a real carved rock) compounds their tiny floating-point
/// disagreement into an actual gap — this is what a leaky real-world union
/// traced back to. Any pair that isn't a matching diagonal (a standalone
/// triangle, or two unrelated ones landing next to each other) is left alone.
fn merge_quad_pairs(polygons: Vec<Polygon>) -> Vec<Polygon> {
    let mut out = Vec::with_capacity(polygons.len());
    let mut rest = polygons.into_iter();
    while let Some(a) = rest.next() {
        let Some(b) = rest.next() else {
            out.push(a);
            break;
        };
        match merge_diagonal(&a, &b) {
            Some(merged) => out.push(merged),
            None => {
                out.push(a);
                out.push(b);
            }
        }
    }
    out
}

/// If `a` and `b` are two triangles sharing exactly one edge in opposite
/// winding directions and lying on (near enough) the same plane, returns the
/// fused quad in `a`'s winding. `None` leaves both triangles as they were.
fn merge_diagonal(a: &Polygon, b: &Polygon) -> Option<Polygon> {
    const POSITION_EPSILON: f64 = 1e-9;
    let same = |p: DVec3, q: DVec3| (p - q).length_squared() < POSITION_EPSILON * POSITION_EPSILON;
    if a.vertices.len() != 3 || b.vertices.len() != 3 {
        return None;
    }
    if a.plane.normal.dot(b.plane.normal) < 1.0 - 1e-7 || (a.plane.w - b.plane.w).abs() > 1e-6 {
        return None;
    }
    // Find a's edge (s_a -> s_b) whose reverse (s_b -> s_a) is one of b's
    // edges; a's remaining corner and b's remaining corner become the quad's
    // other two corners.
    for i in 0..3 {
        let (s_a, s_b) = (a.vertices[i], a.vertices[(i + 1) % 3]);
        let apex_a = a.vertices[(i + 2) % 3];
        for j in 0..3 {
            if same(b.vertices[j], s_b) && same(b.vertices[(j + 1) % 3], s_a) {
                let apex_b = b.vertices[(j + 2) % 3];
                return Some(Polygon {
                    vertices: vec![apex_a, s_a, apex_b, s_b],
                    plane: a.plane,
                });
            }
        }
    }
    None
}

impl Solid {
    /// `mesh`'s triangles placed by `model`; a mirroring matrix has its winding
    /// restored so the solid still faces outward.
    pub(super) fn from_mesh(mesh: &MeshData, model: Mat4) -> Self {
        let model = model.as_dmat4();
        let mirrored = model.determinant() < 0.0;
        let vertex = |index: u32| -> Option<DVec3> {
            let position = mesh.positions.get(index as usize)?;
            Some(model.transform_point3(DVec3::from(Vec3::from(*position))))
        };
        let polygons = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|triangle| {
                let (a, b, c) = (
                    vertex(triangle[0])?,
                    vertex(triangle[1])?,
                    vertex(triangle[2])?,
                );
                let (b, c) = if mirrored { (c, b) } else { (b, c) };
                let plane = Plane::from_points(a, b, c)?;
                Some(Polygon {
                    vertices: vec![a, b, c],
                    plane,
                })
            })
            .collect();
        Solid {
            polygons: merge_quad_pairs(polygons),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub(super) fn union(self, other: Solid) -> Result<Solid, Failure> {
        let mut a = BspNode::from_polygons(self.polygons);
        let mut b = BspNode::from_polygons(other.polygons);
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        Solid::bounded(a.all_polygons())
    }

    pub(super) fn subtract(self, other: Solid) -> Result<Solid, Failure> {
        let mut a = BspNode::from_polygons(self.polygons);
        let mut b = BspNode::from_polygons(other.polygons);
        a.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        a.invert();
        Solid::bounded(a.all_polygons())
    }

    /// Only caps the polygon count here — welding and the watertightness
    /// check are `O(vertices)` to `O(edges * vertices)` costs that a tree of
    /// hundreds of leaves would otherwise pay on every intermediate step;
    /// [`evaluate`] runs them exactly once, on the finished result.
    fn bounded(polygons: Vec<Polygon>) -> Result<Solid, Failure> {
        if polygons.len() > MAX_POLYGONS {
            return Err(Failure::TooComplex);
        }
        Ok(Solid { polygons })
    }

    /// Fan-triangulates every polygon with its plane normal on each corner:
    /// flat shading, the look Roblox's own baked unions have.
    pub(super) fn to_mesh(&self) -> rbx_mesh::Mesh {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for polygon in &self.polygons {
            let base = vertices.len() as u32;
            let normal = polygon.plane.normal.as_vec3().to_array();
            let (axis_u, axis_v) = dominant_axes(normal);
            for vertex in &polygon.vertices {
                let position = vertex.as_vec3().to_array();
                for axis in 0..3 {
                    min[axis] = min[axis].min(position[axis]);
                    max[axis] = max[axis].max(position[axis]);
                }
                vertices.push(rbx_mesh::Vertex {
                    position,
                    normal,
                    // Box projection along whichever pair of axes the facet
                    // actually faces (see `dominant_axes`): every corner got
                    // the same fixed (0, 0) here before, so a whole facet
                    // read from one texel instead of tiling like a real
                    // part's surface does.
                    uv: [
                        position[axis_u] / rbx_materials::DEFAULT_STUDS_PER_TILE,
                        position[axis_v] / rbx_materials::DEFAULT_STUDS_PER_TILE,
                    ],
                    color: [255; 4],
                });
            }
            for corner in 1..polygon.vertices.len().saturating_sub(1) as u32 {
                indices.extend_from_slice(&[base, base + corner, base + corner + 1]);
            }
        }
        rbx_mesh::Mesh {
            // Never read from a file, so there is no header version to echo.
            version: (0, 0),
            vertices,
            indices,
            lods: Vec::new(),
            bounds: rbx_mesh::Aabb { min, max },
        }
    }

    /// Signed volume by the divergence theorem: positive for an outward-wound,
    /// closed solid, negative if the whole result came out inside-out — the
    /// production check in [`evaluate`] as well as a test assertion.
    pub(super) fn volume(&self) -> f64 {
        self.polygons
            .iter()
            .map(|polygon| {
                let first = polygon.vertices[0];
                polygon.vertices[1..]
                    .windows(2)
                    .map(|pair| first.dot(pair[0].cross(pair[1])))
                    .sum::<f64>()
            })
            .sum::<f64>()
            / 6.0
    }
}

/// The two position axes a facet's texture runs along: whichever pair is
/// perpendicular to the *dominant* component of its normal, the standard
/// triplanar "biggest normal axis" rule. Picking anything else risks
/// projecting a near edge-on facet along its own thin axis, stretching the
/// texture into a sliver.
fn dominant_axes(normal: [f32; 3]) -> (usize, usize) {
    let dominant = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .expect("normal has 3 components");
    match dominant {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The unit mesh a leaf's shape draws as, the same generators the renderer
/// instances (`renderer::geometry`), so a carved sphere matches a drawn one.
fn unit_mesh(kind: ShapeKind) -> MeshData {
    match kind {
        ShapeKind::Box => shapes::block(),
        ShapeKind::Ball => shapes::sphere(),
        ShapeKind::CylinderX => shapes::cylinder_x(),
        ShapeKind::CylinderY => shapes::cylinder_y(),
        ShapeKind::Wedge => shapes::wedge(),
        ShapeKind::CornerWedge => shapes::corner_wedge(),
        ShapeKind::Truss {
            axis,
            segments,
            style,
        } => shapes::oriented(shapes::truss(segments, style), axis),
    }
}

/// Evaluates one operation tree in the union's own frame: at every node the
/// additive children are unioned, then each negated child is carved out.
///
/// Leaf meshes are generated once per shape kind and reused across the tree.
pub(super) fn evaluate(root: &Node) -> Result<Solid, Failure> {
    if root.leaf_count() > MAX_LEAVES {
        return Err(Failure::TooComplex);
    }
    let mut cache = Vec::new();
    let solid = evaluate_node(root, &mut cache)?;
    if solid.is_empty() {
        return Err(Failure::Empty);
    }
    let polygons = weld_t_junctions(solid.polygons);
    if !is_watertight(&polygons) {
        return Err(Failure::Leaky);
    }
    // A cut so aggressive it slices a corner off entirely (rather than just
    // notching it) is legitimate CSG output, not a bug — a handful of small
    // disconnected specks scattered around the real result reads as broken
    // geometry, though, not a rock with a chipped corner. See
    // `discard_disconnected_debris` for what stays and what goes.
    let polygons = discard_disconnected_debris(polygons);
    let solid = Solid { polygons };
    let volume = solid.volume();
    // A union can only ever shrink from subtracting; it can never exceed the
    // sum of its additive leaves' own volumes (inclusion-exclusion), so that
    // sum is a cheap ceiling without a second full boolean.
    let ceiling = additive_volume_bound(root);
    if volume <= 0.0 || volume > ceiling + EPSILON_VOLUME {
        return Err(Failure::Inverted);
    }
    Ok(solid)
}

/// Small enough that no legitimate result's own floating-point volume noise
/// trips the [`Failure::Inverted`] ceiling check above.
const EPSILON_VOLUME: f64 = 1e-6;

/// See [`evaluate`]'s ceiling check: the volume every additive leaf would
/// have on its own, ignoring negation and overlap.
fn additive_volume_bound(node: &Node) -> f64 {
    match node {
        Node::Leaf(leaf) if !leaf.negate => {
            let mesh = unit_mesh(leaf.geometry.kind);
            let scale = Mat4::from_scale(leaf.geometry.size);
            Solid::from_mesh(&mesh, scale).volume().abs()
        }
        Node::Leaf(_) => 0.0,
        Node::Operation { children, .. } => children
            .iter()
            .filter(|child| !child.is_negate())
            .map(additive_volume_bound)
            .sum(),
    }
}

/// Groups `polygons` by shared-edge adjacency (union-find), then keeps: the
/// single largest group with positive signed volume (the main shell), and
/// *every* group with non-positive volume, however small. A negative-volume
/// group is a surface facing inward — a fully enclosed cavity, like a
/// negation sitting entirely inside the additive solid — which is a
/// topologically required complement to the shell it hollows out, not
/// debris; dropping it would silently undo the carve it represents. Only
/// same-signed (positive) groups compete against each other to be kept.
/// `is_watertight` still holds afterward: every discarded group is its own
/// individually closed piece, so removing it only removes matched edge
/// pairs, never leaves one side dangling.
fn discard_disconnected_debris(polygons: Vec<Polygon>) -> Vec<Polygon> {
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
    // A negative-volume component is an inward-facing surface — a fully
    // enclosed cavity (the classic case: a negation entirely inside the
    // additive solid, like a bubble) rather than a piece of the outer shell.
    // It is topologically required, however small: dropping it would silently
    // undo the carve it represents. Only a same-signed (positive) component
    // competes to be kept; every negative one always survives.
    let mut volume: HashMap<usize, f64> = HashMap::new();
    let mut positive_size: HashMap<usize, usize> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let root = find(&mut parent, i);
        let first = polygon.vertices[0];
        let v: f64 = polygon.vertices[1..]
            .windows(2)
            .map(|pair| first.dot(pair[0].cross(pair[1])))
            .sum();
        *volume.entry(root).or_insert(0.0) += v / 6.0;
    }
    for i in 0..polygons.len() {
        let root = find(&mut parent, i);
        if volume[&root] > 0.0 {
            *positive_size.entry(root).or_insert(0) += 1;
        }
    }
    let largest_positive = positive_size
        .iter()
        .max_by_key(|&(_, &count)| count)
        .map(|(&root, _)| root);
    polygons
        .into_iter()
        .enumerate()
        .filter(|(i, _)| {
            let root = find(&mut parent, *i);
            volume[&root] <= 0.0 || Some(root) == largest_positive
        })
        .map(|(_, polygon)| polygon)
        .collect()
}

/// Test-only: the same shared-edge grouping [`discard_disconnected_debris`]
/// does, without the filtering — so a test can assert a result is (or, for
/// `discard_disconnected_debris`'s own tests, was) a single connected piece.
#[cfg(test)]
pub(super) fn connected_components(solid: &Solid) -> Vec<f64> {
    let key = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let polygons = &solid.polygons;
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
    let mut volume: HashMap<usize, f64> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let root = find(&mut parent, i);
        let first = polygon.vertices[0];
        let v: f64 = polygon.vertices[1..]
            .windows(2)
            .map(|pair| first.dot(pair[0].cross(pair[1])))
            .sum();
        *volume.entry(root).or_insert(0.0) += v / 6.0;
    }
    volume.into_values().collect()
}

/// Snap tolerance for the edge-adjacency check below: coarser than any
/// legitimate vertex spacing this codebase's shapes produce, fine enough not
/// to merge genuinely distinct nearby edges.
const WELD_EPSILON: f64 = 1e-4;

/// A vertex position snapped to the `WELD_EPSILON` grid, used to recognize
/// "the same point" (or edge) across independently-computed fragments.
type GridPoint = (i64, i64, i64);
type DirectedEdge = (GridPoint, GridPoint);

/// A fragment is finalized as soon as `clip_polygons` confirms it lies in
/// front of just one of the other solid's planes — correct, since that alone
/// proves it is outside a convex cutter, but it means a neighboring fragment
/// that needed more plane tests can have their shared boundary edge
/// subdivided on its side and not on this one. Splits every polygon edge at
/// any other polygon's vertex that lies exactly on it (using that vertex's
/// own value, not a recomputed one, so the two sides end up bit-identical)
/// to turn that T-junction back into a matching pair of edges before
/// [`is_watertight`] ever runs.
fn weld_t_junctions(polygons: Vec<Polygon>) -> Vec<Polygon> {
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

/// A lone stray edge stays invisible at render distance — the risk this
/// guards against is a shattered, hole-ridden mesh, not a single hairline
/// sliver. Anything past this fraction of the mesh's own edges reads as
/// broken rather than merely imperfect (the additive-only fallback is safer
/// past that point); below it, [`weld_t_junctions`] has already done what it
/// can and the rest is the BSP split's inherent floating-point residue.
const MAX_LEAK_FRACTION: f64 = 0.02;

/// Every directed polygon edge in a correctly closed, consistently-wound
/// solid has its exact reverse somewhere else exactly once — this is the
/// invariant a leak (open edge) or overlap (duplicated edge) breaks. Called
/// once, by [`evaluate`], on the finished result.
fn is_watertight(polygons: &[Polygon]) -> bool {
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
    let bad = directed
        .iter()
        .filter(|(&(a, b), &count)| !(count == 1 && directed.get(&(b, a)).copied() == Some(1)))
        .count();
    (bad as f64) <= (directed.len() as f64) * MAX_LEAK_FRACTION
}

fn evaluate_node(node: &Node, cache: &mut Vec<(ShapeKind, MeshData)>) -> Result<Solid, Failure> {
    match node {
        Node::Leaf(leaf) => {
            let kind = leaf.geometry.kind;
            let slot = match cache.iter().position(|(known, _)| *known == kind) {
                Some(slot) => slot,
                None => {
                    cache.push((kind, unit_mesh(kind)));
                    cache.len() - 1
                }
            };
            Ok(Solid::from_mesh(&cache[slot].1, leaf.model()))
        }
        Node::Operation { children, .. } => {
            let mut positive: Option<Solid> = None;
            let mut negatives = Vec::new();
            for child in children {
                let solid = evaluate_node(child, cache)?;
                if solid.is_empty() {
                    continue;
                }
                if child.is_negate() {
                    negatives.push(solid);
                } else {
                    positive = Some(match positive {
                        Some(so_far) => so_far.union(solid)?,
                        None => solid,
                    });
                }
            }
            let Some(mut result) = positive else {
                return Ok(Solid::default());
            };
            for negative in negatives {
                result = result.subtract(negative)?;
            }
            Ok(result)
        }
    }
}

/// The leaf colour a union shows when `UsePartColor` is off: Roblox keeps
/// each original piece's own colour then, but this module's one mesh can
/// only carry a single tint (no per-facet provenance survives the boolean),
/// so it picks the additive leaf with the *largest volume* — the one most
/// likely to dominate what a viewer actually sees — rather than whichever
/// leaf happens to come first in the tree's own (arbitrary, DOM-order)
/// traversal, which would let a small differently-coloured accent piece
/// override the colour of the lump it sits on just by decoding first.
pub(super) fn largest_additive_color(root: &Node) -> Option<[u8; 3]> {
    let mut cache = Vec::new();
    let mut best: Option<(f64, [u8; 3])> = None;
    visit_additive_leaves(root, &mut cache, &mut best);
    best.map(|(_, color)| color)
}

fn visit_additive_leaves(
    node: &Node,
    cache: &mut Vec<(ShapeKind, MeshData)>,
    best: &mut Option<(f64, [u8; 3])>,
) {
    match node {
        Node::Leaf(leaf) if !leaf.negate => {
            let Some(&Variant::Color3uint8 { r, g, b }) = leaf.properties.get("Color3uint8") else {
                return;
            };
            let kind = leaf.geometry.kind;
            let slot = match cache.iter().position(|(known, _)| *known == kind) {
                Some(slot) => slot,
                None => {
                    cache.push((kind, unit_mesh(kind)));
                    cache.len() - 1
                }
            };
            let volume = Solid::from_mesh(&cache[slot].1, leaf.model())
                .volume()
                .abs();
            if !matches!(best, Some((seen, _)) if *seen >= volume) {
                *best = Some((volume, [r, g, b]));
            }
        }
        Node::Leaf(_) => {}
        Node::Operation { negate: true, .. } => {}
        Node::Operation { children, .. } => {
            for child in children {
                visit_additive_leaves(child, cache, best);
            }
        }
    }
}

#[cfg(test)]
#[path = "csg/tests.rs"]
mod tests;
