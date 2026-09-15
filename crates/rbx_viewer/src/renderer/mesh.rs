//! The unit cube every `ShapeKind::Box` part instances, and the vertex format
//! every unit mesh shares (see [`super::geometry`] for the other shapes).

use bytemuck::{Pod, Zeroable};

/// Half the side length of the cube, so a part's `size` can be used as the scale directly.
const HALF: f32 = 0.5;

// (normal, u, v) per face, with `u × v == normal` so the generated quads wind
// counter-clockwise seen from outside and survive back-face culling.
const FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
];

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

/// A vertex: position and per-face normal.
///
/// Each face has 4 vertices with the same normal to ensure flat shading
/// (no interpolation artifacts on box edges).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl Vertex {
    // Lets `renderer::shaped` build vertices from `shapes::MeshData` without this
    // module exposing its private fields.
    pub(super) fn new(position: [f32; 3], normal: [f32; 3]) -> Self {
        Vertex { position, normal }
    }

    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRIBUTES,
        }
    }
}

/// Builds the cube as 24 vertices (four per face, so normals stay flat) and 36 indices.
pub(super) fn cube() -> (Vec<Vertex>, Vec<u16>) {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for (face, (normal, u, v)) in FACES.iter().enumerate() {
        let base = u16::try_from(face).unwrap_or_default() * 4;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(Vertex {
                position: corner(normal, u, v, su, sv),
                normal: *normal,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (vertices, indices)
}

fn corner(normal: &[f32; 3], u: &[f32; 3], v: &[f32; 3], su: f32, sv: f32) -> [f32; 3] {
    std::array::from_fn(|axis| HALF * (normal[axis] + su * u[axis] + sv * v[axis]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cross(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    #[test]
    fn every_face_basis_winds_outwards() {
        for (normal, u, v) in FACES {
            assert_eq!(cross(&u, &v), normal, "face {normal:?} winds inwards");
        }
    }

    #[test]
    fn the_cube_spans_exactly_one_stud() {
        let (vertices, indices) = cube();

        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        for vertex in &vertices {
            for axis in vertex.position {
                assert_eq!(axis.abs(), HALF);
            }
        }
    }
}
