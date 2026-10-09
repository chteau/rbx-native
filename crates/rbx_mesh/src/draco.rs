//! The Draco-compressed `COREMESH` payload of `version 7.00`.
//!
//! Roblox encodes its vertex record as five Draco attributes: `Position`, the
//! normal as a 3 x `f32` `Generic`, `TexCoord`, the tangent as a 4 x `u8`
//! `Generic`, and `Color`. Faces use sequential connectivity, so their order (and
//! with it the `LODS` offsets) survives the round trip. UVs are stored in the same
//! top-left convention as the uncompressed records and are passed through as is.

use draco_core::{DataType, DecoderBuffer, GeometryAttributeType, MeshDecoder, PointAttribute};

use crate::error::MeshError;
use crate::mesh::Vertex;

pub(crate) fn decode(blob: &[u8]) -> Result<(Vec<Vertex>, Vec<[u32; 3]>), MeshError> {
    let mut mesh = draco_core::Mesh::new();
    MeshDecoder::new()
        .decode(&mut DecoderBuffer::new(blob), &mut mesh)
        .map_err(|err| MeshError::Draco(err.to_string()))?;

    let count = mesh.num_points();
    let channel = |attribute: Option<&PointAttribute>, components| {
        attribute.map(|attribute| attribute.read_f32s(count, components))
    };
    let attributes: Vec<&PointAttribute> = (0..mesh.num_attributes())
        .map(|id| mesh.attribute(id))
        .collect();
    // Draco's named `Normal` would be octahedrally quantized; Roblox keeps full
    // precision by declaring it generic. Accept either.
    let normal = mesh
        .named_attribute(GeometryAttributeType::Normal)
        .or_else(|| {
            attributes.iter().copied().find(|attribute| {
                attribute.attribute_type() == GeometryAttributeType::Generic
                    && attribute.data_type() == DataType::Float32
                    && attribute.num_components() == 3
            })
        });

    let positions = channel(mesh.named_attribute(GeometryAttributeType::Position), 3)
        .ok_or(MeshError::MissingPositions)?;
    let normals = channel(normal, 3);
    let uvs = channel(mesh.named_attribute(GeometryAttributeType::TexCoord), 2);
    let colors = channel(mesh.named_attribute(GeometryAttributeType::Color), 4);

    let vertices = (0..count)
        .map(|point| Vertex {
            position: std::array::from_fn(|axis| positions[point * 3 + axis]),
            normal: normals.as_ref().map_or([0.0; 3], |normals| {
                std::array::from_fn(|axis| normals[point * 3 + axis])
            }),
            uv: uvs.as_ref().map_or([0.0; 2], |uvs| {
                std::array::from_fn(|axis| uvs[point * 2 + axis])
            }),
            // Same default as a 36-byte record: white leaves the base color alone.
            color: colors.as_ref().map_or([u8::MAX; 4], |colors| {
                std::array::from_fn(|channel| colors[point * 4 + channel] as u8)
            }),
        })
        .collect();
    let faces = mesh
        .faces()
        .iter()
        .map(|face| face.map(|point| point.0))
        .collect();
    Ok((vertices, faces))
}

#[cfg(test)]
pub(crate) mod tests {
    use draco_core::{EncoderBuffer, EncoderOptions, FaceIndex, MeshEncoder, PointIndex};

    use super::*;

    /// Encodes `vertices` the way Roblox does (see the module docs), or with
    /// positions alone when `full` is false.
    pub(crate) fn encode(vertices: &[Vertex], faces: &[[u32; 3]], full: bool) -> Vec<u8> {
        let mut mesh = draco_core::Mesh::new();
        let mut add = |kind, data_type, components, bytes: Vec<u8>| {
            let mut attribute = PointAttribute::new();
            attribute.init(kind, components, data_type, false, vertices.len());
            attribute.buffer_mut().write(0, &bytes);
            mesh.add_attribute(attribute);
        };
        let floats = |pick: &dyn Fn(&Vertex) -> Vec<f32>| -> Vec<u8> {
            vertices
                .iter()
                .flat_map(pick)
                .flat_map(f32::to_le_bytes)
                .collect()
        };

        use GeometryAttributeType::{Color, Generic, Position, TexCoord};
        add(
            Position,
            DataType::Float32,
            3,
            floats(&|v| v.position.to_vec()),
        );
        if full {
            add(
                Generic,
                DataType::Float32,
                3,
                floats(&|v| v.normal.to_vec()),
            );
            add(TexCoord, DataType::Float32, 2, floats(&|v| v.uv.to_vec()));
            add(Generic, DataType::Uint8, 4, vec![0x7F; vertices.len() * 4]);
            let colors = vertices.iter().flat_map(|vertex| vertex.color).collect();
            add(Color, DataType::Uint8, 4, colors);
        }

        mesh.set_num_faces(faces.len());
        for (index, face) in faces.iter().enumerate() {
            mesh.set_face(FaceIndex(index as u32), face.map(PointIndex));
        }

        let mut options = EncoderOptions::new();
        // Sequential connectivity, as Roblox writes it: edgebreaker would reorder
        // the faces.
        options.set_encoding_method(0);
        let mut encoder = MeshEncoder::new();
        encoder.set_mesh(mesh);
        let mut buffer = EncoderBuffer::new();
        encoder.encode(&options, &mut buffer).unwrap();
        buffer.data().to_vec()
    }

    pub(crate) fn quad() -> (Vec<Vertex>, Vec<[u32; 3]>) {
        let vertices = (0..4u8)
            .map(|corner| {
                let (x, y) = (f32::from(corner & 1), f32::from(corner >> 1));
                Vertex {
                    position: [x * 2.0 - 1.0, y * 0.5, -0.25],
                    normal: [0.0, 0.6, -0.8],
                    uv: [x, 1.0 - y],
                    color: [10 * corner, 20, 30, 255],
                }
            })
            .collect();
        (vertices, vec![[0, 1, 2], [2, 1, 3]])
    }

    #[test]
    fn reads_the_roblox_attribute_layout_back_exactly() {
        let (vertices, faces) = quad();
        let decoded = decode(&encode(&vertices, &faces, true)).unwrap();
        assert_eq!(decoded, (vertices, faces));
    }

    #[test]
    fn absent_attributes_fall_back_like_a_colorless_record() {
        let (vertices, faces) = quad();
        let (decoded, _) = decode(&encode(&vertices, &faces, false)).unwrap();
        assert_eq!(decoded[3].position, vertices[3].position);
        assert_eq!(decoded[3].normal, [0.0; 3]);
        assert_eq!(decoded[3].uv, [0.0; 2]);
        assert_eq!(decoded[3].color, [u8::MAX; 4]);
    }

    #[test]
    fn a_stream_that_is_not_draco_is_an_error() {
        assert!(matches!(decode(b"not draco"), Err(MeshError::Draco(_))));
    }
}
