//! The two vertex formats a file mesh is uploaded in on top of the shape pass's
//! own: UVs for a plain `TextureID`, and UVs plus a tangent frame for a
//! `SurfaceAppearance`, whose normal map is authored in tangent space.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

// The vertex colour sits past the instance attributes (3-11, see
// `instance::INSTANCE_ATTRIBUTES_AFTER_UV`) rather than before them, so the
// instance layout the textured pipeline shares stays where it was.
const TEXTURED_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    12 => Unorm8x4,
];

const APPEARANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];

/// A UV triangle small enough that its tangent is numerical noise: two vertices
/// sharing a texture coordinate, which happens on seams and on degenerate faces.
const MIN_UV_AREA: f32 = 1e-12;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct TexturedVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    /// The mesh file's own RGBA, which only a `ForceField` reads: its alpha
    /// forces the shell solid there (see `filemesh.wgsl`).
    color: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct AppearanceVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    /// xyz: the surface tangent, i.e. where +U runs. w: the handedness that
    /// turns `cross(normal, tangent)` into the bitangent a normal map's green
    /// channel points along.
    tangent: [f32; 4],
}

impl TexturedVertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TEXTURED_ATTRIBUTES,
        }
    }

    pub(super) fn build(mesh: &rbx_mesh::Mesh) -> Vec<TexturedVertex> {
        mesh.vertices
            .iter()
            .map(|vertex| TexturedVertex {
                position: vertex.position,
                normal: vertex.normal,
                uv: vertex.uv,
                color: vertex.color,
            })
            .collect()
    }
}

impl AppearanceVertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<AppearanceVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &APPEARANCE_ATTRIBUTES,
        }
    }

    pub(super) fn build(mesh: &rbx_mesh::Mesh) -> Vec<AppearanceVertex> {
        let tangents = tangents(mesh);
        mesh.vertices
            .iter()
            .zip(tangents)
            .map(|(vertex, tangent)| AppearanceVertex {
                position: vertex.position,
                normal: vertex.normal,
                uv: vertex.uv,
                tangent,
            })
            .collect()
    }
}

/// Generates a per-vertex tangent frame from positions and UVs.
///
/// `rbx_mesh` reads past the 4-byte tangent every mesh record carries: it is
/// all-zero on most files in the wild, so generating is the only thing that
/// works for every mesh rather than for some of them.
///
/// The accumulation is the standard per-triangle one, averaged at shared
/// vertices and orthonormalized against the vertex normal. Its one Roblox
/// specificity is the handedness: mesh `v` grows *downward* (see
/// `rbx_mesh::Vertex`) while a normal map's green channel points *up* the
/// image, so the sign returned is the one that flips `dP/dv` back.
fn tangents(mesh: &rbx_mesh::Mesh) -> Vec<[f32; 4]> {
    let count = mesh.vertices.len();
    let mut along_u = vec![Vec3::ZERO; count];
    let mut along_v = vec![Vec3::ZERO; count];

    for face in mesh.lod0().as_chunks::<3>().0 {
        let corners: Option<Vec<&rbx_mesh::Vertex>> = face
            .iter()
            .map(|&index| mesh.vertices.get(index as usize))
            .collect();
        let Some(corners) = corners else {
            continue;
        };

        let origin = Vec3::from(corners[0].position);
        let edge = [
            Vec3::from(corners[1].position) - origin,
            Vec3::from(corners[2].position) - origin,
        ];
        let duv: [[f32; 2]; 2] = std::array::from_fn(|side| {
            std::array::from_fn(|axis| corners[side + 1].uv[axis] - corners[0].uv[axis])
        });

        let determinant = duv[0][0] * duv[1][1] - duv[1][0] * duv[0][1];
        if determinant.abs() < MIN_UV_AREA {
            continue;
        }
        let scale = 1.0 / determinant;
        let u = (edge[0] * duv[1][1] - edge[1] * duv[0][1]) * scale;
        let v = (edge[1] * duv[0][0] - edge[0] * duv[1][0]) * scale;

        for &index in face {
            along_u[index as usize] += u;
            along_v[index as usize] += v;
        }
    }

    mesh.vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| frame(Vec3::from(vertex.normal), along_u[index], along_v[index]))
        .collect()
}

/// Orthonormalizes one vertex's accumulated frame.
///
/// A vertex no triangle contributed to — or whose contributions cancelled —
/// falls back to any tangent perpendicular to its normal: the normal map still
/// reads flat there, so which one it is does not matter.
fn frame(normal: Vec3, along_u: Vec3, along_v: Vec3) -> [f32; 4] {
    let normal = normal.normalize_or(Vec3::Y);
    let tangent =
        (along_u - normal * normal.dot(along_u)).normalize_or(normal.any_orthonormal_vector());
    // `-along_v` because the bitangent wanted is the one pointing up the image,
    // against the direction mesh `v` grows in.
    let handedness = if normal.cross(tangent).dot(-along_v) < 0.0 {
        -1.0
    } else {
        1.0
    };

    [tangent.x, tangent.y, tangent.z, handedness]
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    /// A unit quad in the XY plane facing +Z, textured so that +U runs along +X
    /// and image-down (+V) runs along -Y — the layout a texture painted on a
    /// wall has.
    fn quad() -> rbx_mesh::Mesh {
        let corner = |x: f32, y: f32, u: f32, v: f32| rbx_mesh::Vertex {
            position: [x, y, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [u, v],
            color: [255; 4],
        };

        rbx_mesh::Mesh {
            version: (4, 1),
            vertices: vec![
                corner(0.0, 1.0, 0.0, 0.0),
                corner(1.0, 1.0, 1.0, 0.0),
                corner(1.0, 0.0, 1.0, 1.0),
                corner(0.0, 0.0, 0.0, 1.0),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            lods: Vec::new(),
            bounds: rbx_mesh::Aabb {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
        }
    }

    #[test]
    fn a_quads_tangent_follows_its_u_axis() {
        for tangent in tangents(&quad()) {
            assert!(Vec3::new(tangent[0], tangent[1], tangent[2]).abs_diff_eq(Vec3::X, 1e-5));
        }
    }

    #[test]
    fn the_generated_frame_is_orthonormal_and_right_handed() {
        let mesh = quad();
        for (vertex, tangent) in mesh.vertices.iter().zip(tangents(&mesh)) {
            let normal = Vec3::from(vertex.normal);
            let axis = Vec3::new(tangent[0], tangent[1], tangent[2]);
            assert!((axis.length() - 1.0).abs() < 1e-5);
            assert!(normal.dot(axis).abs() < 1e-5);

            // Green points up the image, which on this quad is +Y; (T, B, N)
            // is then right-handed, exactly as the shader assumes.
            let bitangent = normal.cross(axis) * tangent[3];
            assert!(bitangent.abs_diff_eq(Vec3::Y, 1e-5));
            assert!(axis.cross(bitangent).abs_diff_eq(normal, 1e-5));
        }
    }

    #[test]
    fn a_vertex_no_triangle_reaches_still_gets_a_usable_tangent() {
        let mut mesh = quad();
        mesh.vertices.push(rbx_mesh::Vertex {
            position: [5.0, 5.0, 5.0],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            color: [255; 4],
        });

        let orphan = *tangents(&mesh).last().expect("one tangent per vertex");
        let axis = Vec3::new(orphan[0], orphan[1], orphan[2]);
        assert!((axis.length() - 1.0).abs() < 1e-5);
        assert!(axis.dot(Vec3::Y).abs() < 1e-5);
    }

    // The tangent rides in the same vertex buffer as the rest, so a field
    // reordered here and not in the WGSL would read neighbouring floats.
    #[test]
    fn every_attribute_addresses_a_field_inside_its_stride() {
        for (attributes, stride) in [
            (
                TEXTURED_ATTRIBUTES.as_slice(),
                std::mem::size_of::<TexturedVertex>(),
            ),
            (
                APPEARANCE_ATTRIBUTES.as_slice(),
                std::mem::size_of::<AppearanceVertex>(),
            ),
        ] {
            for attribute in attributes {
                assert!(attribute.offset + attribute.format.size() <= stride as u64);
            }
        }
        assert_eq!(std::mem::size_of::<AppearanceVertex>(), 48);
        assert_eq!(APPEARANCE_ATTRIBUTES[3].offset, 32);
    }

    #[test]
    fn the_uploaded_vertices_carry_the_meshs_own_uvs() {
        let mesh = quad();
        let built = AppearanceVertex::build(&mesh);

        assert_eq!(built.len(), mesh.vertices.len());
        assert_eq!(built[1].uv, [1.0, 0.0]);
        assert_eq!(Vec4::from(built[1].tangent).w, 1.0);
        assert_eq!(TexturedVertex::build(&mesh)[1].uv, [1.0, 0.0]);
    }

    // What a `ForceField` reads its forced outline from; the shader declares
    // it at the same location (`filemesh.wgsl`'s `VertexInput`).
    #[test]
    fn the_vertex_colour_reaches_the_textured_pipeline_past_the_instance_slots() {
        let mut mesh = quad();
        mesh.vertices[2].color = [10, 20, 30, 40];

        assert_eq!(TexturedVertex::build(&mesh)[2].color, [10, 20, 30, 40]);
        assert_eq!(TEXTURED_ATTRIBUTES[3].shader_location, 12);
        assert_eq!(TEXTURED_ATTRIBUTES[3].offset, 32);
        assert!(super::super::super::pipeline::FILEMESH_SHADER
            .contains("@location(12) color: vec4<f32>"));
    }
}
