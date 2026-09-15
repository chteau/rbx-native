//! Procedural unit meshes for every non-box `BasePart` shape this viewer draws.
//!
//! Each mesh fits exactly inside `[-0.5, 0.5]^3`, the same convention the existing
//! box uses (see `renderer::mesh::cube`), so a part's `size` becomes the model
//! matrix's scale directly. This module has no GPU dependency — every mesh can be
//! validated by a plain unit test — and [`renderer::shaped`](super::renderer)
//! turns a [`MeshData`] into vertex/index buffers.

mod block;
mod corner_wedge;
mod cylinder;
mod sphere;
mod truss;
mod wedge;

pub(crate) use block::block;
pub(crate) use corner_wedge::corner_wedge;
pub(crate) use cylinder::{cylinder_x, cylinder_y};
pub(crate) use sphere::sphere;
pub(crate) use truss::{oriented, truss, TrussAxis, TrussStyle};
pub(crate) use wedge::wedge;

use glam::Vec3;

/// A closed triangle mesh: positions and their per-vertex normals, indexed.
///
/// Every generator in this module builds one of these with [`push_flat_triangle`]
/// or [`push_smooth_triangle`], which orient each triangle outward relative to a
/// point known to sit inside the shape. That means no generator has to
/// hand-derive a winding order — a common source of inside-out geometry — and
/// unit tests can check orientation once, generically, via [`MeshData::volume`].
///
/// [`push_flat_triangle`]: MeshData::push_flat_triangle
/// [`push_smooth_triangle`]: MeshData::push_smooth_triangle
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct MeshData {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) normals: Vec<[f32; 3]>,
    pub(crate) indices: Vec<u32>,
}

impl MeshData {
    /// Appends a flat-shaded triangle: its normal comes from its own plane, not
    /// from a caller-supplied direction, so it can never disagree with the
    /// triangle's own winding once that winding is corrected below.
    fn push_flat_triangle(&mut self, inside: Vec3, a: Vec3, b: Vec3, c: Vec3) {
        let mut positions = [a, b, c];
        let mut normal = (b - a).cross(c - a);
        // A triangle wound the wrong way round has its natural cross-product
        // normal pointing back at the shape's own interior; swapping two
        // corners flips both the winding and the normal to face outward.
        if normal.dot((a + b + c) / 3.0 - inside) < 0.0 {
            positions.swap(1, 2);
            normal = -normal;
        }
        let normal = normal.normalize();
        self.push_triangle(positions, [normal, normal, normal]);
    }

    /// Appends a flat-shaded quad (`a`, `b`, `c`, `d` in order around its
    /// border) as two triangles sharing one normal.
    fn push_flat_quad(&mut self, inside: Vec3, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        self.push_flat_triangle(inside, a, b, c);
        self.push_flat_triangle(inside, a, c, d);
    }

    /// Appends a triangle on a curved surface, carrying its own analytic
    /// per-vertex normals (e.g. the radial direction on a sphere or cylinder).
    ///
    /// Reordered outward exactly like the flat case, but the normals travel
    /// with their own vertex rather than being derived from the winding.
    fn push_smooth_triangle(&mut self, inside: Vec3, positions: [Vec3; 3], normals: [Vec3; 3]) {
        let [a, b, c] = positions;
        let mut positions = positions;
        let mut normals = normals;
        let flat = (b - a).cross(c - a);
        if flat.dot((a + b + c) / 3.0 - inside) < 0.0 {
            positions.swap(1, 2);
            normals.swap(1, 2);
        }
        self.push_triangle(positions, normals.map(Vec3::normalize));
    }

    fn push_triangle(&mut self, positions: [Vec3; 3], normals: [Vec3; 3]) {
        let base = u32::try_from(self.positions.len()).unwrap_or(0);
        for (position, normal) in positions.iter().zip(normals) {
            self.positions.push(position.to_array());
            self.normals.push(normal.to_array());
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    /// The mesh's enclosed volume, signed by winding via the divergence
    /// theorem: `sum(v0 . (v1 x v2)) / 6` over every triangle, independent of
    /// the reference point as long as the surface is closed.
    ///
    /// Positive for a closed, outward-wound mesh — every shape here is convex,
    /// so this doubles as the orientation check every generator's tests use,
    /// and its magnitude can be checked against the shape's known volume.
    ///
    /// Only ever called from tests: production code trusts the orientation
    /// once, here, rather than re-deriving it at render time.
    #[cfg(test)]
    pub(crate) fn volume(&self) -> f32 {
        let vertex = |index: u32| Vec3::from(self.positions[index as usize]);
        let sum: f32 = self
            .indices
            .chunks_exact(3)
            .map(|triangle| {
                let (a, b, c) = (
                    vertex(triangle[0]),
                    vertex(triangle[1]),
                    vertex(triangle[2]),
                );
                a.dot(b.cross(c))
            })
            .sum();
        sum / 6.0
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::MeshData;
    use glam::Vec3;

    /// Every invariant a generated mesh must hold, checked once instead of
    /// once per shape: triangulated, in range, watertight-by-construction
    /// (via [`MeshData::volume`]'s sign), and unit-length normals.
    pub(crate) fn assert_well_formed(mesh: &MeshData) {
        assert!(!mesh.positions.is_empty(), "mesh has no geometry");
        assert_eq!(mesh.positions.len(), mesh.normals.len());
        assert_eq!(mesh.indices.len() % 3, 0, "indices do not form triangles");
        for &index in &mesh.indices {
            assert!(
                (index as usize) < mesh.positions.len(),
                "index out of range"
            );
        }
        for normal in &mesh.normals {
            let length = Vec3::from(*normal).length();
            assert!(
                (length - 1.0).abs() < 1e-4,
                "normal not unit length: {length}"
            );
        }
        assert!(mesh.volume() > 0.0, "mesh winds inward (negative volume)");
    }
}
