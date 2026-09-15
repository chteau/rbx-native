//! Vertex and face records, identical from `version 2.00` onward.

use crate::error::MeshError;
use crate::mesh::Vertex;
use crate::reader::Reader;

/// Position, normal and uv only; the tangent occupies the last four bytes.
const VERTEX_SIZE_PLAIN: u8 = 36;
/// As above plus an RGBA vertex color. The only stride v4 and later ever use.
pub(crate) const VERTEX_SIZE_COLORED: u8 = 40;

pub(crate) const FACE_SIZE: u8 = 12;

const COLOR_OFFSET: usize = 36;

pub(crate) fn check_vertex_size(size: u8) -> Result<(), MeshError> {
    match size {
        VERTEX_SIZE_PLAIN | VERTEX_SIZE_COLORED => Ok(()),
        other => Err(MeshError::UnsupportedVertexSize(other)),
    }
}

pub(crate) fn read_vertices(
    reader: &mut Reader<'_>,
    count: usize,
    stride: u8,
) -> Result<Vec<Vertex>, MeshError> {
    let stride = stride as usize;
    let block = reader.take_records(count, stride)?;
    Ok(block.chunks_exact(stride).map(decode_vertex).collect())
}

pub(crate) fn read_faces(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<[u32; 3]>, MeshError> {
    let stride = FACE_SIZE as usize;
    let block = reader.take_records(count, stride)?;
    Ok(block
        .chunks_exact(stride)
        .map(|face| std::array::from_fn(|corner| u32_at(face, corner * 4)))
        .collect())
}

/// Decodes one vertex record; `record` must be 36 or 40 bytes.
///
/// The 4-byte tangent at offset 32 is read past but not kept: nothing downstream
/// consumes it yet.
/// TODO: it is four *signed* bytes (x, y, z, bitangent sign) decoding as
/// `max(c / 127, -1.0)`. An all-zero word means "absent" in practice, so a consumer
/// that needs tangents should generate them rather than trust the field.
fn decode_vertex(record: &[u8]) -> Vertex {
    Vertex {
        position: std::array::from_fn(|axis| f32_at(record, axis * 4)),
        normal: std::array::from_fn(|axis| f32_at(record, 12 + axis * 4)),
        uv: std::array::from_fn(|axis| f32_at(record, 24 + axis * 4)),
        color: match record.get(COLOR_OFFSET..COLOR_OFFSET + 4) {
            Some(rgba) => rgba.try_into().unwrap(),
            // A 36-byte record carries no tint; white leaves the base color alone.
            None => [u8::MAX; 4],
        },
    }
}

// Both helpers index into a slice whose length `take_records` already validated,
// so the try_into cannot fail.
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_any_stride_but_36_and_40() {
        assert!(check_vertex_size(36).is_ok());
        assert!(check_vertex_size(40).is_ok());
        assert!(matches!(
            check_vertex_size(0),
            Err(MeshError::UnsupportedVertexSize(0))
        ));
        assert!(matches!(
            check_vertex_size(44),
            Err(MeshError::UnsupportedVertexSize(44))
        ));
    }

    #[test]
    fn decodes_a_40_byte_record_including_its_color() {
        let mut record = Vec::new();
        for value in [1.0f32, 2.0, 3.0, 0.0, 0.0, 1.0, 0.5, 0.25] {
            record.extend_from_slice(&value.to_le_bytes());
        }
        record.extend_from_slice(&[0x7F, 0x7F, 0x7F, 0x00]);
        record.extend_from_slice(&[1, 2, 3, 4]);

        let vertex = decode_vertex(&record);
        assert_eq!(vertex.position, [1.0, 2.0, 3.0]);
        assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
        assert_eq!(vertex.uv, [0.5, 0.25]);
        assert_eq!(vertex.color, [1, 2, 3, 4]);

        // The same record truncated to 36 bytes loses only the color.
        assert_eq!(decode_vertex(&record[..36]).color, [255, 255, 255, 255]);
        assert_eq!(decode_vertex(&record[..36]).uv, [0.5, 0.25]);
    }

    #[test]
    fn faces_are_three_little_endian_indices() {
        let mut block = Vec::new();
        for index in [7u32, 8, 9, 0, 1, 2] {
            block.extend_from_slice(&index.to_le_bytes());
        }
        let mut reader = Reader::new(&block);
        assert_eq!(
            read_faces(&mut reader, 2).unwrap(),
            vec![[7, 8, 9], [0, 1, 2]]
        );
    }
}
