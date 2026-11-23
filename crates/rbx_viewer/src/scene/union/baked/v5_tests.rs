//! Version-5 blobs built here in the layout `v5` describes, plus an ignored
//! check over every baked mesh in real places.

use std::collections::HashMap;

use super::v5::WIDE;
use super::*;

/// One vertex of a synthetic document: position, unit normal and colour.
type Corner = ([f32; 3], [f32; 3], [u8; 4]);

fn unit(value: f32) -> [u8; 2] {
    (((value * 32767.0).round() as i32 + 32767).clamp(0, 65534) as u16).to_le_bytes()
}

/// The delta stream for `indices`, a wide code wherever a delta does not fit
/// seven bits.
fn stream(indices: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous = 0i64;
    for &index in indices {
        let delta = i64::from(index) - previous;
        previous = i64::from(index);
        if (-64..64).contains(&delta) {
            out.push((delta as i8 as u8) & 0x7f);
        } else {
            let word = (delta as i32 as u32) & 0x7f_ffff;
            out.push(WIDE | (word >> 16) as u8);
            out.extend(((word & 0xffff) as u16).to_be_bytes());
        }
    }
    out
}

/// The plain version-5 document; `lods` are the offsets written at its end.
fn document(corners: &[Corner], indices: &[u32], lods: &[u32]) -> Vec<u8> {
    let count = (corners.len() as u16).to_le_bytes();
    let units = |pick: &dyn Fn(&Corner) -> [f32; 3]| {
        let mut out = count.to_vec();
        out.extend((corners.len() as u32 * 6).to_le_bytes());
        for corner in corners {
            for value in pick(corner) {
                out.extend(unit(value));
            }
        }
        out
    };
    let mut out = MAGIC.to_vec();
    out.extend(5u32.to_le_bytes());
    out.extend(count);
    for (position, ..) in corners {
        position.iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    out.extend(units(&|c| c.1));
    out.extend(count);
    corners.iter().for_each(|c| out.extend(c.2));
    out.extend(count);
    out.extend(std::iter::repeat_n(1u8, corners.len()));
    out.extend(count);
    out.extend(std::iter::repeat_n(0u8, corners.len() * 8));
    out.extend(units(&|c| c.1));
    let encoded = stream(indices);
    out.extend((indices.len() as u32).to_le_bytes());
    out.extend((encoded.len() as u32).to_le_bytes());
    out.extend(encoded);
    out.push(lods.len() as u8);
    lods.iter().for_each(|l| out.extend(l.to_le_bytes()));
    out
}

/// As Studio stores one: the magic and version scrambled, the rest plain.
fn scrambled(plain: &[u8]) -> Vec<u8> {
    let mut out = plain.to_vec();
    for (at, byte) in out.iter_mut().enumerate().take(PREFIX) {
        *byte ^= KEY[at % KEY.len()];
    }
    out
}

/// A quad facing +Y, two triangles, then the same two again as a second
/// LOD, the shape every real version-5 blob has.
fn quad() -> (Vec<Corner>, Vec<u32>, Vec<u32>) {
    let up = [0.0, 1.0, 0.0];
    let green = [20, 200, 40, 255];
    let corners = vec![
        ([-3.0, 0.5, -2.0], up, green),
        ([-3.0, 0.5, 2.0], up, green),
        ([3.0, 0.5, 2.0], up, green),
        ([3.0, 0.5, -2.0], up, green),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3];
    (corners, indices, vec![0, 6, 12])
}

#[test]
fn a_version_five_blob_decodes_its_first_lod() {
    let (corners, indices, lods) = quad();
    let baked = decode(&scrambled(&document(&corners, &indices, &lods))).expect("decodes");
    let mesh = &baked.mesh;

    assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
    assert_eq!(mesh.vertices[2].position, [3.0, 0.5, 2.0]);
    assert_eq!(mesh.vertices[2].normal, [0.0, 1.0, 0.0]);
    assert_eq!(mesh.bounds.min, [-3.0, 0.5, -2.0]);
    assert!(super::tests::has_color(mesh, [20, 200, 40]));
}

#[test]
fn the_normal_encoding_spans_minus_one_to_one() {
    let corners = [
        ([0.0; 3], [-1.0, 0.0, 0.0], [0; 4]),
        ([1.0; 3], [0.0, 0.0, 1.0], [0; 4]),
        ([2.0; 3], [0.0, 1.0, 0.0], [0; 4]),
    ];
    let baked = decode(&scrambled(&document(&corners, &[0, 1, 2], &[]))).expect("decodes");
    let normals: Vec<_> = baked.mesh.vertices.iter().map(|v| v.normal).collect();

    assert_eq!(
        normals,
        [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]]
    );
}

#[test]
fn a_delta_too_wide_for_seven_bits_takes_the_wide_code() {
    let corners: Vec<Corner> = (0..300)
        .map(|i| ([i as f32, 0.0, 0.0], [0.0, 1.0, 0.0], [255; 4]))
        .collect();
    let indices = [0, 1, 299, 299, 298, 0];
    let document = document(&corners, &indices, &[]);
    assert!(
        stream(&indices).iter().any(|b| b & WIDE != 0),
        "the test exercises it"
    );

    let baked = decode(&scrambled(&document)).expect("decodes");
    assert_eq!(baked.mesh.indices, indices);
}

#[test]
fn the_wide_form_reads_the_words_studio_writes() {
    // Words read off real streams: +186, then −185.
    let corners: Vec<Corner> = (0..200)
        .map(|i| ([i as f32, 0.0, 0.0], [0.0, 1.0, 0.0], [255; 4]))
        .collect();
    let mut plain = document(&corners, &[0, 0, 0], &[]);
    let stream_at = plain.len() - 1 - 3;
    plain.splice(
        stream_at..stream_at + 3,
        [0x80, 0x00, 0xba, 0xff, 0xff, 0x47, 0x00],
    );
    plain[stream_at - 4..stream_at].copy_from_slice(&7u32.to_le_bytes());

    let baked = decode(&scrambled(&plain)).expect("decodes");
    assert_eq!(baked.mesh.indices, [186, 1, 1]);
}

#[test]
fn a_wide_code_cut_short_is_truncation() {
    let (corners, ..) = quad();
    let mut plain = document(&corners, &[0, 1, 2], &[]);
    let stream_at = plain.len() - 1 - 3;
    // A wide code needing two more bytes, with only two stream bytes left.
    plain[stream_at + 1] = WIDE;
    plain[stream_at + 2] = WIDE;

    assert_eq!(decode(&scrambled(&plain)).err(), Some(Error::Truncated));
}

#[test]
fn a_stream_longer_than_its_indices_is_refused() {
    let (corners, indices, _) = quad();
    let mut plain = document(&corners, &indices[..6], &[]);
    // One index fewer than the stream holds.
    let count_at = plain.len() - 1 - 6 - 8;
    plain[count_at..count_at + 4].copy_from_slice(&5u32.to_le_bytes());

    assert_eq!(decode(&scrambled(&plain)).err(), Some(Error::Index));
}

#[test]
fn an_index_below_zero_is_refused() {
    let (corners, ..) = quad();
    let mut plain = document(&corners, &[0, 1, 2], &[]);
    let stream_at = plain.len() - 1 - 3;
    plain[stream_at] = 0x7f;

    assert_eq!(decode(&scrambled(&plain)).err(), Some(Error::Index));
}

#[test]
fn arrays_that_disagree_on_the_vertex_count_are_refused() {
    let (corners, indices, lods) = quad();
    let mut plain = document(&corners, &indices, &lods);
    // The normals' count, right after the four positions.
    let at = PREFIX + 2 + 4 * 12;
    plain[at..at + 2].copy_from_slice(&3u16.to_le_bytes());

    assert_eq!(decode(&scrambled(&plain)).err(), Some(Error::Count));
}

#[test]
fn every_truncation_is_an_error_never_a_panic() {
    let (corners, indices, lods) = quad();
    let blob = scrambled(&document(&corners, &indices, &lods));
    for len in 0..blob.len() {
        assert!(decode(&blob[..len]).is_err(), "cut at {len}");
    }
}

/// Every `MeshData`/`MeshData2` blob in `RBX_UNION_SURVEY_FIXTURE`'s places
/// (colon-separated), whatever its version: each must decode to a closed
/// mesh that spans the `InitialSize` its union was baked at and, where the
/// union also carries its tree, holds the volume this project carves from it.
#[test]
#[ignore = "needs RBX_UNION_SURVEY_FIXTURE (colon-separated .rbxl paths)"]
fn every_baked_mesh_in_real_places_decodes_closed_and_fills_its_bake() {
    let paths = std::env::var("RBX_UNION_SURVEY_FIXTURE").expect("RBX_UNION_SURVEY_FIXTURE");
    let mut versions: HashMap<u32, usize> = HashMap::new();
    let mut checked: HashMap<u32, usize> = HashMap::new();
    let database = rbx_reflection::ReflectionDatabase::embedded();
    for path in paths.split(':') {
        let bytes = std::fs::read(path).expect("fixture must be readable");
        let dom = rbx_binary::deserialize(&bytes).expect("fixture must parse");
        for referent in crate::scene::descendants(&dom) {
            let properties = dom.get(referent).expect("walked").properties();
            let Some(blob) = mesh_data(properties) else {
                continue;
            };
            let version = {
                let document = plain(blob).expect("a CSGMDL blob");
                u32::from_le_bytes(document[6..10].try_into().expect("four bytes"))
            };
            *versions.entry(version).or_default() += 1;
            let baked = decode(blob).unwrap_or_else(|e| panic!("{path}: v{version}: {e:?}"));
            assert_closed(&baked.mesh, path);
            // Ground truth where the union also carries its tree: the
            // boolean carved from it here. Version 2 tessellated round parts
            // a little coarser than `csg` does, hence the slack.
            if let Some(carved) = carved_volume(properties, &database) {
                let decoded = volume(&baked.mesh);
                assert!(
                    (decoded / carved - 1.0).abs() <= 0.02,
                    "{path}: v{version} holds {decoded}, its tree carves {carved}"
                );
                *checked.entry(version).or_default() += 1;
            }
            if let Some(rbx_dom::Variant::Vector3(initial)) = properties.get("InitialSize") {
                let initial = [initial.x, initial.y, initial.z];
                let bounds = &baked.mesh.bounds;
                for (axis, baked_at) in initial.into_iter().enumerate() {
                    let extent = bounds.max[axis] - bounds.min[axis];
                    assert!(
                        (extent - baked_at).abs() <= 0.02 * baked_at.max(0.2),
                        "{path}: v{version} spans {extent} on axis {axis}, baked at {baked_at}"
                    );
                }
            }
        }
    }
    println!("decoded by version: {versions:?}, volume checked: {checked:?}");
    assert!(!versions.is_empty(), "the places hold baked meshes");
}

/// Every edge shared by exactly two triangles, once each way, welding
/// corners by position (a bake splits vertices along hard edges).
pub(super) fn assert_closed(mesh: &rbx_mesh::Mesh, path: &str) {
    let key = |i: u32| {
        mesh.vertices[i as usize]
            .position
            .map(|c| (c * 1e3).round() as i64)
    };
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for triangle in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (a, b) = (key(triangle[a]), key(triangle[b]));
            if a == b {
                continue;
            }
            *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
        }
    }
    let open = edges.values().filter(|&&n| n != 0).count();
    assert_eq!(open, 0, "{path}: {open} unmatched edges");
}

/// Signed volume by the divergence theorem: positive for an outward-wound
/// closed mesh.
pub(super) fn volume(mesh: &rbx_mesh::Mesh) -> f32 {
    let at = |i: u32| glam::Vec3::from(mesh.vertices[i as usize].position);
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[a, b, c]| at(a).dot(at(b).cross(at(c))) / 6.0)
        .sum()
}

/// The volume of the boolean this project carves from the union's own tree,
/// where it carries one inline and the boolean runs.
fn carved_volume(
    properties: &std::collections::BTreeMap<String, rbx_dom::Variant>,
    database: &rbx_reflection::ReflectionDatabase,
) -> Option<f32> {
    let raw = super::super::tree::child_data(properties)?;
    let bake = match properties.get("InitialSize") {
        Some(rbx_dom::Variant::Vector3(v)) => Some(glam::Vec3::new(v.x, v.y, v.z)),
        _ => None,
    };
    let parsed = super::super::legacy::parse_as_baked(raw, database, &Default::default(), bake)?;
    if !parsed.missing.is_empty() {
        return None;
    }
    let solid = super::super::csg::evaluate(&parsed.root, bake).ok()?;
    Some(solid.volume() as f32)
}
