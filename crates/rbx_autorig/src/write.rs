//! Output formats: Roblox's `version 2.00` mesh, for previewing a part
//! locally before upload; its skinned `version 4.00` mesh, for previewing a
//! skinned part; and one `.glb` of all 15 parts for Open Cloud's model
//! import, rigid or skinned.

use glam::{Vec2, Vec3};
use serde_json::{json, Value};

use crate::mesh::Mesh;
use crate::rig::{Joint, Part};
use crate::skin::{PartSkin, Skin};
use crate::split::Body;

/// The mesh as a Roblox `version 2.00` file: a 12-byte header, 40-byte
/// vertices (position, normal, UV, an absent tangent, opaque white) and
/// 12-byte faces, little-endian. Missing normals are written as +Y and
/// missing UVs as zero.
pub fn mesh_v2(mesh: &Mesh) -> Vec<u8> {
    let n = mesh.positions.len();
    let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
    let mut out = Vec::with_capacity(13 + 12 + n * 40 + triangles.len() * 12);
    out.extend_from_slice(b"version 2.00\n");
    out.extend_from_slice(&12u16.to_le_bytes());
    out.push(40);
    out.push(12);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for i in 0..n {
        let p = mesh.positions[i];
        let normal = mesh.normals.get(i).copied().unwrap_or(Vec3::Y);
        let uv = mesh.uvs.get(i).copied().unwrap_or(Vec2::ZERO);
        for v in [p.x, p.y, p.z, normal.x, normal.y, normal.z, uv.x, uv.y] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&[255; 4]);
    }
    for t in triangles {
        for i in t {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    out
}

/// Appends `bytes` to the binary buffer as a buffer view (4-byte aligned)
/// and an accessor; returns the accessor index.
#[allow(clippy::too_many_arguments)]
fn accessor(
    bin: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    bytes: &[u8],
    component: u32,
    count: usize,
    kind: &str,
    target: Option<u32>,
    bounds: Option<(Vec3, Vec3)>,
) -> usize {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut view = json!({
        "buffer": 0,
        "byteOffset": bin.len(),
        "byteLength": bytes.len(),
    });
    if let Some(target) = target {
        view["target"] = json!(target);
    }
    views.push(view);
    bin.extend_from_slice(bytes);
    let mut a = json!({
        "bufferView": views.len() - 1,
        "componentType": component,
        "count": count,
        "type": kind,
    });
    if let Some((lo, hi)) = bounds {
        a["min"] = json!([lo.x, lo.y, lo.z]);
        a["max"] = json!([hi.x, hi.y, hi.z]);
    }
    accessors.push(a);
    accessors.len() - 1
}

fn floats<const N: usize>(items: impl Iterator<Item = [f32; N]>) -> Vec<u8> {
    items.flatten().flat_map(f32::to_le_bytes).collect()
}

/// A point or direction of Roblox space as an uploaded glTF holds it: turned
/// half round about Y, so the character faces +Z, glTF's front. Roblox's
/// importer turns what it reads half round to face −Z: written facing −Z,
/// models 95056738325785 and 91357319063637 came back facing +Z, and the
/// skinned one then stood back to front in game however its parts were
/// turned afterwards. A turn, not a mirror, so faces keep their winding.
pub(crate) fn file_space(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.y, -v.z)
}

/// All 15 parts as one binary glTF: a node per part named `<Part>_Geo` (the
/// importer's naming for body parts) at the part's centre, holding a mesh of
/// the same name. Coordinates are Roblox space in studs, +Y up, turned to
/// face +Z ([`file_space`]). Roblox's importer reads glTF units as
/// studs: on live uploads (rigid model 95056738325785, skinned
/// 91357319063637) every part's `InitialSize` equalled its glTF bounds.
pub fn gltf(body: &Body) -> Vec<u8> {
    let mut buffer = Buffer::default();
    let (mut meshes, mut nodes) = (Vec::new(), Vec::new());
    for piece in &body.parts {
        let name = format!("{}_Geo", piece.part.name());
        let primitive = buffer.primitive(&piece.mesh, Vec3::ZERO);
        meshes.push(json!({ "name": name, "primitives": [primitive] }));
        nodes.push(json!({
            "name": name,
            "mesh": meshes.len() - 1,
            "translation": file_space(piece.centre).to_array(),
        }));
    }
    let document = json!({
        "asset": { "version": "2.0", "generator": "rbx-native autorig" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": buffer.accessors,
        "bufferViews": buffer.views,
    });
    glb(document, buffer.bin)
}

/// The binary chunk being built, with its views and accessors.
#[derive(Default)]
struct Buffer {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffer {
    fn accessor(
        &mut self,
        bytes: &[u8],
        component: u32,
        count: usize,
        kind: &str,
        target: Option<u32>,
        bounds: Option<(Vec3, Vec3)>,
    ) -> usize {
        accessor(
            &mut self.bin,
            &mut self.views,
            &mut self.accessors,
            bytes,
            component,
            count,
            kind,
            target,
            bounds,
        )
    }

    /// `mesh` moved by `offset` as a triangle-list primitive in
    /// [`file_space`]: positions, normals and UVs when present, and indices.
    fn primitive(&mut self, mesh: &Mesh, offset: Vec3) -> Value {
        let n = mesh.positions.len();
        let positions: Vec<Vec3> = mesh
            .positions
            .iter()
            .map(|&p| file_space(p + offset))
            .collect();
        let bounds = crate::mesh::bounds(positions.iter().copied());
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "POSITION".into(),
            json!(self.accessor(
                &floats(positions.iter().map(|p| p.to_array())),
                5126,
                n,
                "VEC3",
                Some(34962),
                Some(bounds),
            )),
        );
        if mesh.normals.len() == n {
            attributes.insert(
                "NORMAL".into(),
                json!(self.accessor(
                    &floats(mesh.normals.iter().map(|&p| file_space(p).to_array())),
                    5126,
                    n,
                    "VEC3",
                    Some(34962),
                    None,
                )),
            );
        }
        if mesh.uvs.len() == n {
            attributes.insert(
                "TEXCOORD_0".into(),
                json!(self.accessor(
                    &floats(mesh.uvs.iter().map(|p| p.to_array())),
                    5126,
                    n,
                    "VEC2",
                    Some(34962),
                    None,
                )),
            );
        }
        let indices: Vec<u8> = mesh
            .valid_triangles()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect();
        let indices = self.accessor(
            &indices,
            5125,
            indices.len() / 4,
            "SCALAR",
            Some(34963),
            None,
        );
        json!({ "attributes": attributes, "indices": indices, "mode": 4 })
    }
}

/// Frames a glTF document and its binary buffer as a `.glb`, declaring the
/// buffer in the document.
fn glb(mut document: Value, mut bin: Vec<u8>) -> Vec<u8> {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    document["buffers"] = json!([{ "byteLength": bin.len() }]);
    let mut text = serde_json::to_vec(&document).expect("JSON values always serialise");
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// Bone indices a subset may list (Roblox's limit).
const SUBSET_BONES: usize = 26;

/// The mesh as a Roblox `version 4.00` file, skinned by `skin` when given.
/// Little-endian throughout:
///
/// - `"version 4.00\n"`.
/// - Header, 24 bytes: `u16` header size (24); `u16` LOD type, 0 ("none":
///   nothing was simplified); `u32` vertex count; `u32` face count; `u16`
///   LOD offset count, 2 (one level: `[0, faces]`); `u16` bone count; `u32`
///   size of the bone name table; `u16` subset count (1 when skinned, else
///   0); `u8` high-quality LOD count, 1 (the one level is full quality);
///   `u8` unused, 0. Roblox's own skinned meshes carry several LOD levels
///   and say how they were made: the R15 body mesh 13674780763 (v5) has
///   LOD type 3 (Zeux) with 4 levels, accessory v4.01 meshes type 4 with 6;
///   both count 1 high-quality level, as ours does. With a single level
///   nothing was simplified, so type 0 is the truthful value; Roblox
///   re-processes uploads into its own mesh assets anyway (the glTF route
///   in [`gltf`]/[`gltf_skinned`] is what Publish uses).
/// - Vertices, 40 bytes each: `f32` position ×3, normal ×3 (missing: +Y),
///   UV ×2 (missing: 0); `i8` tangent ×4, all 0 (none computed); `u8`
///   RGBA, opaque white.
/// - Skinned only, an envelope per vertex, 8 bytes: four `u8` bone slots
///   (indices into the subset's bone list, which here is the bone table in
///   order, so a slot is also the bone's index), then four `u8` weights
///   summing to 255. Unused slots repeat the first slot with weight 0.
/// - Faces, `u32` ×3 each.
/// - LOD offsets, `u32` ×2: `0`, face count.
/// - Bones, 60 bytes each, in [`PartSkin::bones`] order (parents first):
///   `u32` offset of the name in the name table; `u16` parent (index of the
///   bone's R15 parent part in this table, `0xFFFF` if it is not listed);
///   `u16` LOD parent, the same value, as Roblox's own files have it; `f32`
///   culling radius, 0 as on Roblox's structural bones; the bind CFrame as
///   Roblox stores it, in the part's mesh space: `f32` ×9 rotation rows
///   (identity) then `f32` ×3 position ([`PartSkin::binds`]).
/// - The name table: each bone's part name, NUL-terminated.
/// - One subset, 72 bytes: `u32` first face (0), face count, first vertex
///   (0), vertex count, number of bones listed; `u16` ×26 bone indices
///   (`0..n`, then `0xFFFF`).
///
/// # Panics
///
/// If `skin` has a different number of vertices from `mesh`.
pub fn mesh_v4(mesh: &Mesh, skin: Option<&PartSkin>) -> Vec<u8> {
    let n = mesh.positions.len();
    let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
    if let Some(skin) = skin {
        assert_eq!(skin.influences.len(), n, "the skin belongs to another mesh");
        assert!(skin.bones.len() <= SUBSET_BONES);
    }
    let bones = skin.map_or(&[][..], |s| s.bones.as_slice());
    let mut names = Vec::new();
    let mut name_offsets = Vec::with_capacity(bones.len());
    for bone in bones {
        name_offsets.push(names.len() as u32);
        names.extend_from_slice(bone.name().as_bytes());
        names.push(0);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"version 4.00\n");
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(bones.len() as u16).to_le_bytes());
    out.extend_from_slice(&(names.len() as u32).to_le_bytes());
    out.extend_from_slice(&u16::from(skin.is_some()).to_le_bytes());
    out.push(1);
    out.push(0);
    for i in 0..n {
        let p = mesh.positions[i];
        let normal = mesh.normals.get(i).copied().unwrap_or(Vec3::Y);
        let uv = mesh.uvs.get(i).copied().unwrap_or(Vec2::ZERO);
        for v in [p.x, p.y, p.z, normal.x, normal.y, normal.z, uv.x, uv.y] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&[255; 4]);
    }
    if let Some(skin) = skin {
        for influence in &skin.influences {
            out.extend(influence.iter().map(|e| e.0));
            out.extend(influence.iter().map(|e| e.1));
        }
    }
    for t in &triangles {
        for i in t {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    if let Some(skin) = skin {
        for (k, bone) in skin.bones.iter().enumerate() {
            let parent = bone
                .parent()
                .and_then(|p| skin.bones.iter().position(|&b| b == p))
                .map_or(0xFFFF, |i| i as u16);
            out.extend_from_slice(&name_offsets[k].to_le_bytes());
            out.extend_from_slice(&parent.to_le_bytes());
            out.extend_from_slice(&parent.to_le_bytes());
            out.extend_from_slice(&0f32.to_le_bytes());
            let p = skin.binds[k];
            for v in [1., 0., 0., 0., 1., 0., 0., 0., 1., p.x, p.y, p.z] {
                out.extend_from_slice(&f32::to_le_bytes(v));
            }
        }
        out.extend_from_slice(&names);
        for v in [0, triangles.len() as u32, 0, n as u32, bones.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for k in 0..SUBSET_BONES {
            let index = if k < bones.len() { k as u16 } else { 0xFFFF };
            out.extend_from_slice(&index.to_le_bytes());
        }
    }
    out
}

/// All 15 parts as one skinned binary glTF, for Open Cloud's model import
/// of a skinned R15 body.
///
/// - Nodes 0 and 1 are `Root` and `HumanoidRootNode`, then one node per
///   part named after the part (`LowerTorso`, `UpperTorso`, …) in
///   [`Part::ALL`] order, parented as R15 parts are (`Root →
///   HumanoidRootNode → LowerTorso → …`). Each sits at its bone's joint
///   (the joint whose `Part1` it is), with identity rotation and scale.
/// - Coordinates are Roblox space (+Y up, studs) moved so the Root joint is
///   at the origin and turned to face +Z ([`file_space`]), as the importer requires ("the
///   LowerTorso and Root bone position must be 0, 0, 0"): `Root`,
///   `HumanoidRootNode` and `LowerTorso` all sit at 0, 0, 0, and a point of
///   the file is at `file_space(world − skin.joints[Root])`.
/// - Then 15 nodes named `<Part>_Geo`, each holding the part's mesh of the
///   same name (vertices in that shifted space) and skinned to the one skin,
///   whose joints are the 17 bone nodes and whose inverse bind matrices are
///   the inverses of their (translation-only) world transforms.
/// - `JOINTS_0` is unsigned bytes (indices into the skin's joints, never
///   `Root` or `HumanoidRootNode` with a non-zero weight), `WEIGHTS_0`
///   floats, each [`PartSkin`] byte weight / 255, so every vertex sums to 1.
pub fn gltf_skinned(body: &Body, skin: &Skin) -> Vec<u8> {
    let root = skin.joints[Joint::Root as usize];
    let bone_world = |part: Part| skin.bone_position(part) - root;
    let mut buffer = Buffer::default();
    let mut nodes = vec![
        json!({ "name": "Root", "children": [1] }),
        json!({ "name": "HumanoidRootNode", "children": [2 + Part::LowerTorso as usize] }),
    ];
    for part in Part::ALL {
        let parent = part.parent().map_or(Vec3::ZERO, bone_world);
        let mut node = json!({
            "name": part.name(),
            "translation": file_space(bone_world(part) - parent).to_array(),
        });
        let children: Vec<usize> = Part::ALL
            .into_iter()
            .filter(|c| c.parent() == Some(part))
            .map(|c| 2 + c as usize)
            .collect();
        if !children.is_empty() {
            node["children"] = json!(children);
        }
        nodes.push(node);
    }
    let joints = nodes.len();
    let inverse_binds: Vec<u8> = floats((0..joints).map(|j| {
        let at = if j < 2 {
            Vec3::ZERO
        } else {
            file_space(bone_world(Part::ALL[j - 2]))
        };
        let mut m = [0f32; 16];
        (m[0], m[5], m[10], m[15]) = (1., 1., 1., 1.);
        (m[12], m[13], m[14]) = (-at.x, -at.y, -at.z);
        m
    }));
    let inverse_binds = buffer.accessor(&inverse_binds, 5126, joints, "MAT4", None, None);

    let mut meshes = Vec::new();
    for (piece, part_skin) in body.parts.iter().zip(&skin.parts) {
        let name = format!("{}_Geo", piece.part.name());
        let mut primitive = buffer.primitive(&piece.mesh, piece.centre - root);
        let n = piece.mesh.positions.len();
        assert_eq!(
            part_skin.influences.len(),
            n,
            "the skin belongs to another body"
        );
        let joint_bytes: Vec<u8> = part_skin
            .influences
            .iter()
            .flat_map(|inf| inf.map(|(slot, _)| 2 + part_skin.bones[slot as usize] as u8))
            .collect();
        let weights = floats(
            part_skin
                .influences
                .iter()
                .map(|inf| inf.map(|(_, w)| f32::from(w) / 255.)),
        );
        primitive["attributes"]["JOINTS_0"] =
            json!(buffer.accessor(&joint_bytes, 5121, n, "VEC4", Some(34962), None));
        primitive["attributes"]["WEIGHTS_0"] =
            json!(buffer.accessor(&weights, 5126, n, "VEC4", Some(34962), None));
        meshes.push(json!({ "name": name, "primitives": [primitive] }));
        nodes.push(json!({ "name": name, "mesh": meshes.len() - 1, "skin": 0 }));
    }
    let mut scene = vec![0];
    scene.extend(joints..nodes.len());
    let document = json!({
        "asset": { "version": "2.0", "generator": "rbx-native autorig" },
        "scene": 0,
        "scenes": [{ "nodes": scene }],
        "nodes": nodes,
        "meshes": meshes,
        "skins": [{
            "name": "Armature",
            "joints": (0..joints).collect::<Vec<_>>(),
            "skeleton": 0,
            "inverseBindMatrices": inverse_binds,
        }],
        "accessors": buffer.accessors,
        "bufferViews": buffer.views,
    });
    glb(document, buffer.bin)
}
