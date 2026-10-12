//! Skinning data shared by the `version 4.00`–`5.00` tail and the v6/v7
//! `SKINNING` chunk: per-vertex envelopes, the bone table with its name buffer,
//! and the subsets that map envelope slots to global bone ids.
//!
//! The raw records are identical in both layouts (8-byte envelope, 60-byte bone,
//! 72-byte subset); only the counts around them differ, so the readers here take
//! counts from the caller.

use std::ops::Range;

use crate::error::MeshError;
use crate::reader::Reader;

/// `u8` bone slots plus `u8` weights, per vertex.
pub(crate) const ENVELOPE_SIZE: usize = 8;
pub(crate) const BONE_SIZE: usize = 60;
pub(crate) const SUBSET_SIZE: usize = 72;

/// Bone slots a subset can address; the subset record always holds this many
/// `u16`s, padded with `0xFFFF`.
pub const MAX_SUBSET_BONES: usize = 26;

const NO_BONE: u16 = 0xFFFF;

/// One bone of a skinned mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    /// Name from the file's NUL-terminated name table. Roblox binds a bone to
    /// the part or `Bone` instance of the same name in the character.
    pub name: String,
    /// Index of the parent bone in [`Skin::bones`], `None` for a root.
    pub parent: Option<u16>,
    /// The second hierarchy field; equals `parent` on every file observed.
    pub lod_parent: Option<u16>,
    /// A culling radius; 0 for structural bones.
    pub culling: f32,
    /// Rotation as stored, row-major (`r00, r01, r02, r10, …`).
    pub rotation: [f32; 9],
    /// Position as stored.
    pub position: [f32; 3],
}

impl Bone {
    /// The bone's bind pose as a CFrame in the mesh's own (part-local, unscaled)
    /// space: `[x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22]`, the
    /// argument order of `CFrame.new`. The file stores it in that space, not
    /// relative to the parent bone.
    pub fn cframe(&self) -> [f32; 12] {
        let mut out = [0.0; 12];
        out[..3].copy_from_slice(&self.position);
        out[3..].copy_from_slice(&self.rotation);
        out
    }
}

/// A contiguous run of faces and vertices sharing one table of up to
/// [`MAX_SUBSET_BONES`] bones.
#[derive(Debug, Clone, PartialEq)]
pub struct Subset {
    pub faces: Range<u32>,
    pub vertices: Range<u32>,
    /// Global bone ids addressed by envelope slots `0..bones.len()`. Padding
    /// entries past the declared count are dropped; a `0xFFFF` within it is kept
    /// as written and resolves to no bone.
    pub bones: Vec<u16>,
}

/// The raw per-vertex envelope as stored: four subset-local slots, four weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    pub slots: [u8; 4],
    pub weights: [u8; 4],
}

/// Up to four resolved influences of one vertex.
///
/// `bones[k]` is a global index into [`Skin::bones`] and `weights[k]` its weight
/// in `0..=1` (the stored byte over 255). Slots that are unused, or whose bone
/// could not be resolved, carry weight 0 and bone 0. Weights are not
/// renormalized; on real files they sum to exactly 255/255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Influences {
    pub bones: [u16; 4],
    pub weights: [f32; 4],
}

impl Influences {
    /// The influences with a non-zero weight, as `(global bone, weight)`.
    pub fn iter(&self) -> impl Iterator<Item = (u16, f32)> + '_ {
        self.bones
            .iter()
            .zip(&self.weights)
            .filter(|(_, weight)| **weight > 0.0)
            .map(|(bone, weight)| (*bone, *weight))
    }
}

/// Skinning data of a mesh that declares bones.
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    pub bones: Vec<Bone>,
    pub subsets: Vec<Subset>,
    /// Raw envelopes, one per vertex (the file's own vertex order).
    pub envelopes: Vec<Envelope>,
    /// Resolved influences, one per vertex, parallel to [`crate::Mesh::vertices`].
    pub influences: Vec<Influences>,
}

impl Skin {
    /// Index of the bone named `name`, if any.
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|bone| bone.name == name)
    }

    /// Builds a skin from its raw parts, resolving every envelope slot through
    /// the subset that contains its vertex.
    ///
    /// A vertex in no subset, a slot past its subset's table, a `0xFFFF` entry
    /// or a bone id past the table all resolve to "no influence" rather than an
    /// error: the geometry is still drawable, merely unskinned there.
    pub fn new(bones: Vec<Bone>, subsets: Vec<Subset>, envelopes: Vec<Envelope>) -> Skin {
        let influences = envelopes
            .iter()
            .enumerate()
            .map(|(vertex, envelope)| resolve(vertex as u32, envelope, &subsets, bones.len()))
            .collect();
        Skin {
            bones,
            subsets,
            envelopes,
            influences,
        }
    }
}

fn resolve(vertex: u32, envelope: &Envelope, subsets: &[Subset], bone_count: usize) -> Influences {
    let mut out = Influences {
        bones: [0; 4],
        weights: [0.0; 4],
    };
    let Some(subset) = subsets
        .iter()
        .find(|subset| subset.vertices.contains(&vertex))
    else {
        return out;
    };
    for k in 0..4 {
        let weight = envelope.weights[k];
        if weight == 0 {
            continue;
        }
        let Some(&bone) = subset.bones.get(envelope.slots[k] as usize) else {
            continue;
        };
        if bone == NO_BONE || bone as usize >= bone_count {
            continue;
        }
        out.bones[k] = bone;
        out.weights[k] = weight as f32 / 255.0;
    }
    out
}

pub(crate) fn read_envelopes(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<Envelope>, MeshError> {
    let block = reader.take_records(count, ENVELOPE_SIZE)?;
    Ok(block
        .as_chunks::<ENVELOPE_SIZE>()
        .0
        .iter()
        .map(|record| Envelope {
            slots: record[..4].try_into().unwrap(),
            weights: record[4..].try_into().unwrap(),
        })
        .collect())
}

/// Bone records with their name offsets still unresolved; the name table comes
/// after them in both layouts.
pub(crate) struct RawBones(Vec<(u32, Bone)>);

pub(crate) fn read_bones(reader: &mut Reader<'_>, count: usize) -> Result<RawBones, MeshError> {
    let block = reader.take_records(count, BONE_SIZE)?;
    Ok(RawBones(
        block
            .as_chunks::<BONE_SIZE>()
            .0
            .iter()
            .map(|record| {
                let parent = |offset| match u16_at(record, offset) {
                    NO_BONE => None,
                    index => Some(index),
                };
                let floats: [f32; 13] = std::array::from_fn(|i| f32_at(record, 8 + i * 4));
                let bone = Bone {
                    name: String::new(),
                    parent: parent(4),
                    lod_parent: parent(6),
                    culling: floats[0],
                    rotation: floats[1..10].try_into().unwrap(),
                    position: floats[10..13].try_into().unwrap(),
                };
                (u32_at(record, 0), bone)
            })
            .collect(),
    ))
}

impl RawBones {
    /// Names each bone from `table`. An offset past the table yields an empty
    /// name rather than an error: the name only matters for binding.
    pub(crate) fn named(self, table: &[u8]) -> Vec<Bone> {
        self.0
            .into_iter()
            .map(|(offset, mut bone)| {
                let tail = table.get(offset as usize..).unwrap_or_default();
                let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
                bone.name = String::from_utf8_lossy(&tail[..end]).into_owned();
                bone
            })
            .collect()
    }
}

pub(crate) fn read_subsets(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<Subset>, MeshError> {
    let block = reader.take_records(count, SUBSET_SIZE)?;
    Ok(block
        .as_chunks::<SUBSET_SIZE>()
        .0
        .iter()
        .map(|record| {
            let word = |i: usize| u32_at(record, i * 4);
            let range = |begin: u32, len: u32| begin..begin.saturating_add(len);
            let declared = (word(4) as usize).min(MAX_SUBSET_BONES);
            Subset {
                faces: range(word(0), word(1)),
                vertices: range(word(2), word(3)),
                bones: (0..declared).map(|i| u16_at(record, 20 + i * 2)).collect(),
            }
        })
        .collect())
}

// Callers index into records whose length `take_records` already validated.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Test-only encoders for the three skinning records, shared by the v4 and
/// chunked tests (and usable as a reference for a writer).
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn envelope_bytes(envelopes: &[Envelope]) -> Vec<u8> {
        envelopes
            .iter()
            .flat_map(|e| e.slots.into_iter().chain(e.weights))
            .collect()
    }

    /// Encodes `bones`, returning the bone records and the name table.
    pub(crate) fn bone_bytes(bones: &[Bone]) -> (Vec<u8>, Vec<u8>) {
        let mut records = Vec::new();
        let mut names = Vec::new();
        for bone in bones {
            records.extend_from_slice(&(names.len() as u32).to_le_bytes());
            names.extend_from_slice(bone.name.as_bytes());
            names.push(0);
            for parent in [bone.parent, bone.lod_parent] {
                records.extend_from_slice(&parent.unwrap_or(NO_BONE).to_le_bytes());
            }
            let floats = std::iter::once(bone.culling)
                .chain(bone.rotation)
                .chain(bone.position);
            records.extend(floats.flat_map(f32::to_le_bytes));
        }
        (records, names)
    }

    pub(crate) fn subset_bytes(subsets: &[Subset]) -> Vec<u8> {
        let mut out = Vec::new();
        for subset in subsets {
            for word in [
                subset.faces.start,
                subset.faces.end - subset.faces.start,
                subset.vertices.start,
                subset.vertices.end - subset.vertices.start,
                subset.bones.len() as u32,
            ] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            for slot in 0..MAX_SUBSET_BONES {
                let bone = subset.bones.get(slot).copied().unwrap_or(NO_BONE);
                out.extend_from_slice(&bone.to_le_bytes());
            }
        }
        out
    }

    pub(crate) fn bone(name: &str, parent: Option<u16>, position: [f32; 3]) -> Bone {
        Bone {
            name: name.to_owned(),
            parent,
            lod_parent: parent,
            culling: 0.0,
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            position,
        }
    }

    /// Two bones, two subsets over four vertices; the second subset lists its
    /// bones in the opposite order so slot 0 means a different bone in each.
    pub(crate) fn sample() -> (Vec<Bone>, Vec<Subset>, Vec<Envelope>) {
        let bones = vec![
            bone("UpperArm", None, [0.0, 1.0, 0.0]),
            bone("LowerArm", Some(0), [0.0, -0.5, 0.0]),
        ];
        let subsets = vec![
            Subset {
                faces: 0..1,
                vertices: 0..2,
                bones: vec![0, 1],
            },
            Subset {
                faces: 1..2,
                vertices: 2..4,
                bones: vec![1, 0],
            },
        ];
        let envelopes = vec![
            Envelope {
                slots: [0, 0, 0, 0],
                weights: [255, 0, 0, 0],
            },
            Envelope {
                slots: [0, 1, 0, 0],
                weights: [128, 127, 0, 0],
            },
            Envelope {
                slots: [0, 1, 0, 0],
                weights: [51, 204, 0, 0],
            },
            Envelope {
                slots: [0, 9, 0, 0],
                weights: [255, 0, 0, 0],
            },
        ];
        (bones, subsets, envelopes)
    }

    #[test]
    fn slots_resolve_through_the_vertex_s_own_subset() {
        let (bones, subsets, envelopes) = sample();
        let skin = Skin::new(bones, subsets, envelopes);

        assert_eq!(skin.influences[0].iter().collect::<Vec<_>>(), [(0, 1.0)]);
        assert_eq!(
            skin.influences[1].iter().collect::<Vec<_>>(),
            [(0, 128.0 / 255.0), (1, 127.0 / 255.0)]
        );
        // Subset 1 lists [1, 0]: slot 0 is bone 1 here.
        assert_eq!(
            skin.influences[2].iter().collect::<Vec<_>>(),
            [(1, 0.2), (0, 0.8)]
        );
        assert_eq!(skin.influences[3].iter().collect::<Vec<_>>(), [(1, 1.0)]);
        assert_eq!(skin.bone("LowerArm"), Some(1));
        assert_eq!(skin.bone("Head"), None);
    }

    #[test]
    fn unresolvable_slots_carry_no_weight() {
        let (bones, mut subsets, _) = sample();
        subsets[0].bones = vec![0xFFFF, 7];
        let envelopes = vec![
            Envelope {
                slots: [0, 1, 5, 0],
                weights: [100, 100, 55, 0],
            };
            4
        ];
        let skin = Skin::new(bones, subsets[..1].to_vec(), envelopes);
        // 0xFFFF, an id past the bone table and a slot past the subset table.
        assert_eq!(skin.influences[0].iter().count(), 0);
        // Vertices 2 and 3 are in no subset at all.
        assert_eq!(skin.influences[3].iter().count(), 0);
    }

    #[test]
    fn records_round_trip_through_the_readers() {
        let (bones, subsets, envelopes) = sample();
        let (bone_records, names) = bone_bytes(&bones);

        let envelope_block = envelope_bytes(&envelopes);
        let subset_block = subset_bytes(&subsets);
        assert_eq!(bone_records.len(), 2 * BONE_SIZE);
        assert_eq!(subset_block.len(), 2 * SUBSET_SIZE);

        assert_eq!(
            read_envelopes(&mut Reader::new(&envelope_block), 4).unwrap(),
            envelopes
        );
        let read = read_bones(&mut Reader::new(&bone_records), 2)
            .unwrap()
            .named(&names);
        assert_eq!(read, bones);
        assert_eq!(read[1].cframe()[..4], [0.0, -0.5, 0.0, 1.0]);
        assert_eq!(
            read_subsets(&mut Reader::new(&subset_block), 2).unwrap(),
            subsets
        );
    }

    #[test]
    fn a_name_offset_past_the_table_is_an_empty_name() {
        let (bones, _, _) = sample();
        let (records, _) = bone_bytes(&bones);
        let read = read_bones(&mut Reader::new(&records), 2)
            .unwrap()
            .named(b"Up\0");
        assert_eq!(read[0].name, "Up");
        assert_eq!(read[1].name, "");
    }
}
