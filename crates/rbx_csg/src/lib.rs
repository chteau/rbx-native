//! Boolean geometry for unions, negations and intersections over closed
//! triangle meshes, a BSP-tree CSG ported from Evan Wallace's csg.js (MIT).
//! Every polygon carries its own plane; a boolean is "clip each solid's
//! polygons against the other's tree, then merge". The viewer carves the
//! unions a place file holds with it, and Local Run and Studio make new ones.
//!
//! Everything is `f64`: the splits chain through dozens of operations, and
//! `f32` classification drifts far enough to open seams along coplanar faces.

pub mod bsp;
pub mod csgmdl;
pub mod csgphs;
pub mod decompose;
pub mod hull;
pub mod repair;
pub mod shapes;
pub mod solidmesh;

pub use glam;
use glam::{DMat4, DVec3, Mat4};

use bsp::{BspNode, Plane, Polygon};
use repair::{
    clean, component_roots, discard_disconnected_debris, is_watertight, weld_t_junctions,
};
pub use shapes::MeshData;

/// A result that outgrows this many polygons after any single step is
/// abandoned: the fallback box is better than a load stalled on one asset.
const MAX_POLYGONS: usize = 200_000;

/// Medium stone grey, a new part's colour: what a face no part painted takes.
pub const DEFAULT_COLOR: [u8; 3] = [163, 162, 165];

/// Why a boolean did not turn into a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    TooComplex,
    /// Every polygon carved away (or none to begin with).
    Empty,
    /// The result still has too many open edges after welding: a BSP split
    /// against a rotated plane can leave a T-junction (`weld_t_junctions`
    /// closes most of these) or, rarely, a genuine gap. Caught here so a
    /// badly leaky result never reaches the renderer or the physics.
    Leaky,
    /// The signed volume is non-positive, or exceeds the sum of every
    /// additive leaf's own volume, a global (or large partial) winding
    /// flip, or a boolean that grew instead of shrank.
    Inverted,
}

/// What [`Solid::mass_properties`] measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub centroid: DVec3,
    /// `Ixx`, `Iyy`, `Izz`.
    pub inertia_diagonal: DVec3,
    /// `Ixy`, `Ixz`, `Iyz` (negated products of inertia).
    pub inertia_off_diagonal: DVec3,
}

/// A closed polygon soup, the unit of every boolean here.
#[derive(Debug, Clone, Default)]
pub struct Solid {
    pub polygons: Vec<Polygon>,
}

/// Re-fuses the two triangles of each flat quad face (every shape generator
/// in [`shapes`] emits a face as two triangles sharing a diagonal, back
/// to back, see `MeshData::push_flat_quad`) back into one 4-vertex polygon.
///
/// The diagonal is purely a rasterizer convenience; feeding the BSP tree a
/// triangulated face instead of the whole flat face gives every cutting plane
/// two independent fragmentation histories for the same physical surface, and
/// a many-cut boolean (a real carved rock) compounds their tiny floating-point
/// disagreement into an actual gap, this is what a leaky real-world union
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
    if a.vertices.len() != 3 || b.vertices.len() != 3 || a.color != b.color {
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
                    color: a.color,
                });
            }
        }
    }
    None
}

impl Solid {
    /// `mesh`'s triangles placed by `model`, see [`Solid::from_triangles`].
    pub fn from_mesh(mesh: &MeshData, model: Mat4) -> Self {
        Solid::from_triangles(&mesh.positions, &mesh.indices, model.as_dmat4())
    }

    /// Indexed triangles placed by `model`; a mirroring matrix has its
    /// winding restored so the solid still faces outward.
    pub fn from_triangles(positions: &[[f32; 3]], indices: &[u32], model: DMat4) -> Self {
        let mirrored = model.determinant() < 0.0;
        let vertex = |index: u32| -> Option<DVec3> {
            let position = positions.get(index as usize)?;
            Some(model.transform_point3(DVec3::from(position.map(f64::from))))
        };
        let polygons = indices
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
                    color: DEFAULT_COLOR,
                })
            })
            .collect();
        Solid {
            polygons: merge_quad_pairs(polygons),
        }
    }

    /// The same solid with every face painted `color`.
    pub fn painted(mut self, color: [u8; 3]) -> Solid {
        for polygon in &mut self.polygons {
            polygon.color = color;
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub fn union(self, other: Solid) -> Result<Solid, Failure> {
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

    pub fn subtract(self, other: Solid) -> Result<Solid, Failure> {
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

    /// What both solids hold, csg.js's `intersect`.
    pub fn intersect(self, other: Solid) -> Result<Solid, Failure> {
        let mut a = BspNode::from_polygons(self.polygons);
        let mut b = BspNode::from_polygons(other.polygons);
        a.invert();
        b.clip_to(&a);
        b.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        a.build(b.all_polygons());
        a.invert();
        Solid::bounded(a.all_polygons())
    }

    /// Only caps the polygon count here, welding and the watertightness
    /// check are `O(vertices)` to `O(edges * vertices)` costs that a tree of
    /// hundreds of leaves would otherwise pay on every intermediate step;
    /// [`Solid::finish`] runs them exactly once, on the finished result.
    fn bounded(polygons: Vec<Polygon>) -> Result<Solid, Failure> {
        if polygons.len() > MAX_POLYGONS {
            return Err(Failure::TooComplex);
        }
        Ok(Solid { polygons })
    }

    /// The repair pass every finished boolean goes through once: welds the
    /// float noise, closes T-junctions, refuses a leaky result and drops
    /// debris outside `bake` (see `repair::discard_disconnected_debris`).
    pub fn finish(self, bake: Option<DVec3>) -> Result<Solid, Failure> {
        if self.is_empty() {
            return Err(Failure::Empty);
        }
        // Cleaned again after welding: a T-junction split can land within the
        // snap tolerance of a corner it already had.
        let polygons = clean(weld_t_junctions(clean(self.polygons)));
        if !is_watertight(&polygons) {
            return Err(Failure::Leaky);
        }
        Ok(Solid {
            polygons: discard_disconnected_debris(polygons, bake),
        })
    }

    /// The solid's separate pieces, as `SplitApart` returns them: every
    /// connected shell, each enclosed cavity staying with the shell around
    /// it. One piece is the whole solid.
    pub fn split_apart(self) -> Vec<Solid> {
        let roots = component_roots(&self.polygons);
        let mut groups: Vec<(usize, Solid)> = Vec::new();
        for (polygon, root) in self.polygons.into_iter().zip(roots) {
            match groups.iter_mut().find(|(r, _)| *r == root) {
                Some((_, group)) => group.polygons.push(polygon),
                None => groups.push((
                    root,
                    Solid {
                        polygons: vec![polygon],
                    },
                )),
            }
        }
        let (mut shells, cavities): (Vec<Solid>, Vec<Solid>) = groups
            .into_iter()
            .map(|(_, group)| group)
            .partition(|group| group.volume() > 0.0);
        for cavity in cavities {
            let Some((lo, hi)) = cavity.bounds() else {
                continue;
            };
            let around = shells.iter().position(|shell| {
                shell
                    .bounds()
                    .is_some_and(|(a, b)| (a.cmple(lo) & b.cmpge(hi)).all())
            });
            match around {
                Some(at) => shells[at].polygons.extend(cavity.polygons),
                None => shells.push(cavity),
            }
        }
        shells
    }

    /// The same solid moved by `model`.
    pub fn transformed(&self, model: DMat4) -> Solid {
        let mirrored = model.determinant() < 0.0;
        let polygons = self
            .polygons
            .iter()
            .map(|polygon| {
                let mut vertices: Vec<DVec3> = polygon
                    .vertices
                    .iter()
                    .map(|&v| model.transform_point3(v))
                    .collect();
                if mirrored {
                    vertices.reverse();
                }
                let plane = Plane::from_points(vertices[0], vertices[1], vertices[2])
                    .unwrap_or(polygon.plane);
                Polygon {
                    vertices,
                    plane,
                    color: polygon.color,
                }
            })
            .collect();
        Solid { polygons }
    }

    /// Fan-triangulates every polygon with its plane normal on each corner:
    /// flat shading, the look Roblox's own baked unions have. Each corner
    /// carries its polygon's colour, what Studio paints a union in when
    /// `UsePartColor` is off.
    pub fn to_mesh(&self) -> rbx_mesh::Mesh {
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
                    color: [polygon.color[0], polygon.color[1], polygon.color[2], 255],
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
    /// closed solid, negative if the whole result came out inside-out.
    pub fn volume(&self) -> f64 {
        self.tetrahedra().map(|(volume, _)| volume).sum::<f64>()
    }

    /// The centre of mass of the solid at uniform density: the
    /// volume-weighted centroids of the tetrahedra the divergence theorem
    /// sums, `None` for a solid with no volume.
    pub fn centroid(&self) -> Option<DVec3> {
        let (volume, moment) = self
            .tetrahedra()
            .fold((0.0, DVec3::ZERO), |(v, m), (dv, c)| (v + dv, m + c * dv));
        (volume.abs() > f64::EPSILON).then(|| moment / volume)
    }

    /// Volume, centre of mass and volume inertia about that centre at unit
    /// density, the `UnscaledVolume`, `UnscaledCofm` and
    /// `UnscaledVolInertiaDiags`/`OffDiags` Studio saves on a union.
    /// `None` for a solid with no volume.
    pub fn mass_properties(&self) -> Option<MassProperties> {
        let mut volume = 0.0;
        let mut moment = DVec3::ZERO;
        // Second moments about the origin: x², y², z², then xy, xz, yz.
        let mut square = DVec3::ZERO;
        let mut product = DVec3::ZERO;
        for polygon in &self.polygons {
            let a = polygon.vertices[0];
            for pair in polygon.vertices[1..].windows(2) {
                let (b, c) = (pair[0], pair[1]);
                let det = a.dot(b.cross(c));
                volume += det / 6.0;
                moment += (a + b + c) * det / 24.0;
                square += (a * a + b * b + c * c + a * b + a * c + b * c) * det / 60.0;
                let mixed = |i: usize, j: usize| {
                    2.0 * (a[i] * a[j] + b[i] * b[j] + c[i] * c[j])
                        + a[i] * b[j]
                        + a[j] * b[i]
                        + a[i] * c[j]
                        + a[j] * c[i]
                        + b[i] * c[j]
                        + b[j] * c[i]
                };
                product += DVec3::new(mixed(0, 1), mixed(0, 2), mixed(1, 2)) * det / 120.0;
            }
        }
        if volume.abs() <= f64::EPSILON {
            return None;
        }
        let centroid = moment / volume;
        let square = square - centroid * centroid * volume;
        let product = product
            - DVec3::new(
                centroid.x * centroid.y,
                centroid.x * centroid.z,
                centroid.y * centroid.z,
            ) * volume;
        Some(MassProperties {
            volume,
            centroid,
            inertia_diagonal: DVec3::new(
                square.y + square.z,
                square.x + square.z,
                square.x + square.y,
            ),
            inertia_off_diagonal: -product,
        })
    }

    /// Axis-aligned bounds, `None` for an empty solid.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut points = self.polygons.iter().flat_map(|p| p.vertices.iter());
        let first = *points.next()?;
        Some(points.fold((first, first), |(lo, hi), &v| (lo.min(v), hi.max(v))))
    }

    /// Each fan triangle's signed tetrahedron against the origin: its
    /// volume and centroid.
    fn tetrahedra(&self) -> impl Iterator<Item = (f64, DVec3)> + '_ {
        self.polygons.iter().flat_map(|polygon| {
            let first = polygon.vertices[0];
            polygon.vertices[1..].windows(2).map(move |pair| {
                let volume = first.dot(pair[0].cross(pair[1])) / 6.0;
                (volume, (first + pair[0] + pair[1]) / 4.0)
            })
        })
    }
}

/// The two position axes a facet's texture runs along: whichever pair is
/// perpendicular to the *dominant* component of its normal, the standard
/// triplanar "biggest normal axis" rule. Picking anything else risks
/// projecting a near edge-on facet along its own thin axis, stretching the
/// texture into a sliver.
pub fn dominant_axes(normal: [f32; 3]) -> (usize, usize) {
    let dominant = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .expect("normal has 3 components");
    match dominant {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

#[cfg(test)]
mod tests;
