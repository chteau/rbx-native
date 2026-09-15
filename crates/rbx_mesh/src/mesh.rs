//! In-memory mesh representation shared by every version parser.

use std::ops::Range;

use crate::error::MeshError;

/// A single mesh vertex.
///
/// `uv` is normalized to one convention for every mesh version: the origin is the
/// **top-left** of the texture and `v` grows downward. That is how `version 2.00`
/// and later already store it; `version 1.00`/`1.01` store `v` inverted and the
/// parser flips it on the way in, so callers never branch on version. Renderers
/// sampling with a bottom-left origin (OpenGL, glTF, Blender) need `1.0 - v`;
/// wgpu, D3D and Vulkan are top-left already and need no flip.
///
/// `normal` is *not* re-normalized. Hand-authored `version 1.00` meshes exist whose
/// normals have lengths well past 1.0, and silently rescaling them would hide that
/// from a caller that wants to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Vertex tint, opaque white when the source file carries no color channel.
    pub color: [u8; 4],
}

/// Axis-aligned bounding box over every vertex in the file.
///
/// Coarse LODs keep their own private vertex ranges, so this covers vertices that
/// LOD 0 never references. Measured across the sample meshes the difference from
/// the LOD-0-only hull never exceeded 0.04 studs, which is why the cheaper
/// whole-buffer box is what gets reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// The box that contains nothing; degenerate (`min > max`) so that any
    /// subsequent union yields exactly the unioned points.
    fn empty() -> Self {
        Aabb {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    pub fn center(&self) -> [f32; 3] {
        std::array::from_fn(|axis| (self.min[axis] + self.max[axis]) / 2.0)
    }

    pub fn size(&self) -> [f32; 3] {
        std::array::from_fn(|axis| self.max[axis] - self.min[axis])
    }
}

/// A parsed Roblox mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// Literal header version, e.g. `(4, 1)` for `version 4.01`.
    pub version: (u8, u8),
    pub vertices: Vec<Vertex>,
    /// Triangle indices for LOD 0 only.
    ///
    /// Lower LODs are decimations of the same shape that a renderer drawing at
    /// native quality never reads, so they are dropped rather than doubling the
    /// index memory of every mesh. [`Mesh::lods`] still reports their extents.
    pub indices: Vec<u32>,
    /// Face ranges per LOD as declared by the file, finest first. Empty when the
    /// file carries no usable LOD table; `lods[0]` always matches [`Mesh::indices`].
    pub lods: Vec<Range<u32>>,
    pub bounds: Aabb,
}

impl Mesh {
    /// The LOD 0 triangle indices.
    pub fn lod0(&self) -> &[u32] {
        &self.indices
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Assembles a mesh, validating that every index addresses a real vertex.
    ///
    /// `faces` is the complete face list; `lods` is the raw LOD offset table read
    /// from the file (offsets into `faces`, one entry per boundary). LOD 0 is
    /// `[table[0], table[1])`.
    pub(crate) fn assemble(
        version: (u8, u8),
        vertices: Vec<Vertex>,
        faces: &[[u32; 3]],
        lod_table: &[u32],
    ) -> Result<Mesh, MeshError> {
        let vertex_count = vertices.len();
        if let Some(&index) = faces
            .iter()
            .flatten()
            .find(|&&i| i as usize >= vertex_count)
        {
            return Err(MeshError::IndexOutOfRange {
                index,
                vertex_count,
            });
        }

        let lods = lod_ranges(lod_table, faces.len());
        // A degenerate table (see `lod_ranges`) leaves `lods` empty, in which case
        // every face belongs to the single implicit LOD.
        let lod0 = lods.first().cloned().unwrap_or(0..faces.len() as u32);
        let indices = faces[lod0.start as usize..lod0.end as usize]
            .iter()
            .flatten()
            .copied()
            .collect();

        Ok(Mesh {
            version,
            bounds: bounds_of(&vertices),
            vertices,
            indices,
            lods,
        })
    }
}

/// Converts a LOD offset table into face ranges, rejecting tables that cannot
/// describe the mesh.
///
/// Observed in the wild (7 of 29 sample files, all `version 4.01` with
/// `lod_type == 0`): a two-entry table of `[0, 0]` alongside a real face list.
/// Trusting it would render nothing, so any table whose final offset disagrees
/// with the face count is discarded wholesale rather than half-believed.
fn lod_ranges(table: &[u32], face_count: usize) -> Vec<Range<u32>> {
    let usable = table.len() >= 2
        && table.last() == Some(&(face_count as u32))
        && table.windows(2).all(|pair| pair[0] <= pair[1]);
    if !usable {
        return Vec::new();
    }
    table
        .windows(2)
        .map(|pair| pair[0]..pair[1])
        .filter(|range| !range.is_empty())
        .collect()
}

fn bounds_of(vertices: &[Vertex]) -> Aabb {
    vertices.iter().fold(Aabb::empty(), |mut aabb, vertex| {
        for axis in 0..3 {
            aabb.min[axis] = aabb.min[axis].min(vertex.position[axis]);
            aabb.max[axis] = aabb.max[axis].max(vertex.position[axis]);
        }
        aabb
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32) -> Vertex {
        Vertex {
            position: [x, x * 2.0, x * 3.0],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            color: [255; 4],
        }
    }

    #[test]
    fn assemble_keeps_only_the_first_lod_range() {
        let vertices = (0..6).map(|i| vertex(i as f32)).collect();
        let faces = [[0, 1, 2], [3, 4, 5]];
        let mesh = Mesh::assemble((3, 0), vertices, &faces, &[0, 1, 2]).unwrap();

        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.lods, vec![0..1, 1..2]);
        assert_eq!(mesh.lod0(), &[0, 1, 2]);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn a_lod_table_that_ends_before_the_faces_is_discarded() {
        let vertices = (0..6).map(|i| vertex(i as f32)).collect();
        let faces = [[0, 1, 2], [3, 4, 5]];
        let mesh = Mesh::assemble((4, 1), vertices, &faces, &[0, 0]).unwrap();

        assert!(mesh.lods.is_empty());
        assert_eq!(mesh.indices, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_non_monotonic_lod_table_is_discarded() {
        let vertices = (0..6).map(|i| vertex(i as f32)).collect();
        let faces = [[0, 1, 2], [3, 4, 5]];
        let mesh = Mesh::assemble((4, 1), vertices, &faces, &[1, 0, 2]).unwrap();

        assert!(mesh.lods.is_empty());
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn an_out_of_range_index_is_rejected() {
        let vertices = vec![vertex(0.0); 3];
        assert!(matches!(
            Mesh::assemble((2, 0), vertices, &[[0, 1, 9]], &[]),
            Err(MeshError::IndexOutOfRange {
                index: 9,
                vertex_count: 3
            })
        ));
    }

    #[test]
    fn bounds_cover_every_vertex() {
        let vertices = vec![vertex(-1.0), vertex(2.0)];
        let mesh = Mesh::assemble((2, 0), vertices, &[], &[]).unwrap();

        assert_eq!(mesh.bounds.min, [-1.0, -2.0, -3.0]);
        assert_eq!(mesh.bounds.max, [2.0, 4.0, 6.0]);
        assert_eq!(mesh.bounds.size(), [3.0, 6.0, 9.0]);
        assert_eq!(mesh.bounds.center(), [0.5, 1.0, 1.5]);
    }
}
