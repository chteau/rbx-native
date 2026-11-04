//! glTF 2.0 as a `.gltf` document with its binary buffer and its images in
//! files beside it ([`gltf_files`]), or, for a small document, all of it
//! inline as base64 data URIs ([`gltf`]). Studio's own glTF export writes the
//! single embedded file ("a single .gltf file that includes embedded
//! textures", the beta's announcement, DevForum thread 3905928), but a JSON
//! document carrying hundreds of megabytes of base64 is past what a
//! JavaScript viewer or validator can hold as one string, so the editor
//! writes the separate files, which every glTF reader also takes.
//!
//! Like Studio's, the instance tree survives: one node per part and per
//! container above it, named after the instance, each placed relative to
//! its parent by rotation and translation, with the instance's class (and a
//! part's `Material`) in `extras` under Studio's own keys. Where this differs
//! from Studio's output, on purpose: a part's scale is in its vertices rather
//! than on its node, so a mesh is never shared between parts of different
//! sizes (Studio shares one unit cube); `baseColorFactor` is linear, as the
//! specification asks, where Studio writes the sRGB bytes over 255 (which is
//! why its colours come out washed in Blender); metalness and roughness are
//! written rather than left at the specification's metallic default; and
//! Neon, Glass and ForceField are written as the closest PBR extensions say
//! (see [`finish`]) where Studio keeps only the material's name.

use std::collections::{BTreeSet, HashMap};

use base64::Engine as _;
use glam::{Mat4, Quat, Vec3};
use serde_json::{json, Value};

use super::{Export, ExportMesh};
use finish::finish;

mod finish;

pub(super) use finish::{FORCE_FIELD_GLOW, FORCE_FIELD_OPACITY, GLASS_IOR};

// glTF's enumerations, from the 2.0 specification.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const TRIANGLES: u32 = 4;
const LINEAR: u32 = 9729;
const LINEAR_MIPMAP_LINEAR: u32 = 9987;
const REPEAT: u32 = 10497;

/// The whole export as one self-contained `.gltf` document.
pub fn gltf(export: &Export) -> String {
    write(export, None).0
}

/// The `.gltf` document named for `stem`, first, then the `.bin` and the
/// `.png`s it names, all meant for the same directory. Only the `.gltf`
/// keeps `stem` as it is: the files it points at are named from inside it,
/// as URIs, so they keep to characters a URI needs no escaping for.
pub fn gltf_files(export: &Export, stem: &str) -> Vec<(String, Vec<u8>)> {
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let (document, mut files) = write(export, Some(&safe));
    files.insert(0, (format!("{stem}.gltf"), document.into_bytes()));
    files
}

/// The document, and (with `stem`) the files it points at rather than
/// embedding.
fn write(export: &Export, stem: Option<&str>) -> (String, Vec<(String, Vec<u8>)>) {
    let mut files = Vec::new();
    let mut out = Buffers::default();
    // Each part's vertices back in its own frame, which its node places.
    let worlds = node_worlds(export);
    let mut owner = vec![Mat4::IDENTITY; export.meshes.len()];
    for (node, world) in export.nodes.iter().zip(&worlds) {
        for &mesh in &node.meshes {
            owner[mesh] = *world;
        }
    }

    let mut primitives = Vec::new();
    for (index, mesh) in export.meshes.iter().enumerate() {
        let to_local = owner[index].inverse();
        let positions: Vec<f32> = mesh
            .positions
            .iter()
            .flat_map(|p| to_local.transform_point3(Vec3::from(*p)).to_array())
            .collect();
        let turn = |v: [f32; 3]| {
            to_local
                .transform_vector3(Vec3::from(v))
                .normalize_or(Vec3::Y)
        };
        let normals: Vec<f32> = mesh
            .normals
            .iter()
            .flat_map(|n| turn(*n).to_array())
            .collect();
        let position = out.add(&positions, ARRAY_BUFFER, mesh.positions.len(), "VEC3");
        // Required on POSITION by the specification.
        let (min, max) = bounds(&positions);
        out.accessors[position]["min"] = json!(min);
        out.accessors[position]["max"] = json!(max);
        let normal = out.add(&normals, ARRAY_BUFFER, mesh.normals.len(), "VEC3");
        let mut attributes = json!({ "POSITION": position, "NORMAL": normal });
        if !mesh.tangents.is_empty() {
            let tangents: Vec<f32> = mesh
                .tangents
                .iter()
                .flat_map(|t| turn([t[0], t[1], t[2]]).extend(t[3]).to_array())
                .collect();
            attributes["TANGENT"] =
                json!(out.add(&tangents, ARRAY_BUFFER, mesh.tangents.len(), "VEC4"));
        }
        if !mesh.uvs.is_empty() {
            let uvs: Vec<f32> = mesh.uvs.iter().flatten().copied().collect();
            attributes["TEXCOORD_0"] = json!(out.add(&uvs, ARRAY_BUFFER, mesh.uvs.len(), "VEC2"));
        }
        // Carried through the float writer bit for bit, then retyped.
        let indices: Vec<f32> = mesh.indices.iter().map(|&i| f32::from_bits(i)).collect();
        let indices = out.add(&indices, ELEMENT_ARRAY_BUFFER, mesh.indices.len(), "SCALAR");
        out.accessors[indices]["componentType"] = json!(UNSIGNED_INT);
        primitives.push(json!({
            "attributes": attributes,
            "indices": indices,
            "material": index,
            "mode": TRIANGLES,
        }));
    }

    let mut gltf_meshes = Vec::new();
    let nodes: Vec<Value> = export
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut out = json!({ "name": node.name });
            let parent = node.parent.map_or(Mat4::IDENTITY, |p| worlds[p]);
            let (_, rotation, translation) = (parent.inverse() * worlds[index]).to_scale_rotation_translation();
            if rotation.angle_between(Quat::IDENTITY) > 1e-6 {
                out["rotation"] = json!(rotation.normalize().to_array());
            }
            if translation != Vec3::ZERO {
                out["translation"] = json!(translation.to_array());
            }
            let children: Vec<usize> = (0..export.nodes.len())
                .filter(|&child| export.nodes[child].parent == Some(index))
                .collect();
            if !children.is_empty() {
                out["children"] = json!(children);
            }
            if !node.meshes.is_empty() {
                gltf_meshes.push(json!({
                    "name": node.name,
                    "primitives": node.meshes.iter().map(|&m| primitives[m].clone()).collect::<Vec<_>>(),
                }));
                out["mesh"] = json!(gltf_meshes.len() - 1);
            }
            let mut extras = json!({ "RobloxInstanceType": node.class });
            if let Some(material) = &node.material {
                extras["Material"] = json!(material);
            }
            out["extras"] = extras;
            out
        })
        .collect();

    // Texture `n` is image `n`, in the order materials first name them; an
    // `Export::textures` entry nothing here names is left out.
    let mut images = Vec::new();
    let mut image_of = HashMap::new();
    let mut texture = |index: usize| {
        *image_of.entry(index).or_insert_with(|| {
            let png = &export.textures[index];
            let uri = match stem {
                Some(stem) => {
                    let name = format!("{stem}_{index}.png");
                    files.push((name.clone(), png.clone()));
                    name
                }
                None => format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                ),
            };
            images.push(json!({ "uri": uri }));
            images.len() - 1
        })
    };
    let mut used = BTreeSet::new();
    let materials: Vec<Value> = export
        .meshes
        .iter()
        .map(|mesh| material(mesh, &mut texture, &mut used))
        .collect();
    let textures: Vec<Value> = (0..images.len())
        .map(|index| json!({ "source": index, "sampler": 0 }))
        .collect();

    let buffer_uri = match stem {
        Some(stem) => {
            let name = format!("{stem}.bin");
            files.push((name.clone(), std::mem::take(&mut out.bytes)));
            name
        }
        None => format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&out.bytes)
        ),
    };
    let byte_length = files
        .last()
        .filter(|_| stem.is_some())
        .map_or(out.bytes.len(), |(_, bytes)| bytes.len());
    let mut document = json!({
        "asset": { "version": "2.0", "generator": "rbxstudio" },
        "scene": 0,
        "scenes": [{
            "nodes": (0..export.nodes.len()).filter(|&n| export.nodes[n].parent.is_none()).collect::<Vec<_>>(),
        }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        "accessors": out.accessors,
        "bufferViews": out.views,
        "buffers": [{
            "byteLength": byte_length,
            "uri": buffer_uri,
        }],
    });
    // The specification forbids an empty array where one may be left out.
    if !images.is_empty() {
        document["textures"] = json!(textures);
        // Trilinear and repeating, as the viewport samples every map: a
        // viewer then picks a mip level per pixel the way it does, and a
        // pack tiled across a face wraps.
        document["samplers"] = json!([{
            "magFilter": LINEAR,
            "minFilter": LINEAR_MIPMAP_LINEAR,
            "wrapS": REPEAT,
            "wrapT": REPEAT,
        }]);
        document["images"] = json!(images);
    }
    if !used.is_empty() {
        document["extensionsUsed"] = json!(used);
    }
    if let Some(document) = document.as_object_mut() {
        for key in ["meshes", "materials", "accessors", "bufferViews"] {
            if document[key].as_array().is_some_and(Vec::is_empty) {
                document.remove(key);
            }
        }
        if export.meshes.is_empty() {
            document.remove("buffers");
            files.retain(|(name, _)| !name.ends_with(".bin"));
        }
    }
    // `json!` cannot fail to serialize: every key is a string and every
    // number a finite one (a NaN would have become `null`).
    (
        serde_json::to_string_pretty(&document).unwrap_or_default(),
        files,
    )
}

/// The one binary buffer, its views and their accessors.
#[derive(Default)]
struct Buffers {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffers {
    /// Appends `values` as a view of its own and an accessor over it,
    /// returning the accessor's index. Every element is four bytes wide, so
    /// each view starts aligned without padding, as the specification
    /// requires of an accessor.
    fn add(&mut self, values: &[f32], target: u32, count: usize, kind: &str) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.views.push(json!({
            "buffer": 0,
            "byteOffset": self.bytes.len(),
            "byteLength": bytes.len(),
            "target": target,
        }));
        self.bytes.extend_from_slice(&bytes);
        self.accessors.push(json!({
            "bufferView": self.views.len() - 1,
            "componentType": FLOAT,
            "count": count,
            "type": kind,
        }));
        self.accessors.len() - 1
    }
}

/// Each node's world placement: a part's own, a container's its parent's
/// (it places nothing itself). Nodes come parent first.
pub(super) fn node_worlds(export: &Export) -> Vec<Mat4> {
    let mut worlds: Vec<Mat4> = Vec::with_capacity(export.nodes.len());
    for node in &export.nodes {
        let world = match node.placement {
            Some((rotation, translation)) => Mat4::from_rotation_translation(
                Quat::from_array(rotation).normalize(),
                Vec3::from(translation),
            ),
            None => node.parent.map_or(Mat4::IDENTITY, |p| worlds[p]),
        };
        worlds.push(world);
    }
    worlds
}

/// The part's colour and maps as a metallic-roughness material, matte and
/// non-metallic unless its maps say otherwise, then its [`finish`].
fn material(
    mesh: &ExportMesh,
    texture: &mut impl FnMut(usize) -> usize,
    used: &mut BTreeSet<&'static str>,
) -> Value {
    let mut material = json!({
        "name": mesh.name,
        "pbrMetallicRoughness": {
            "baseColorFactor": mesh.color,
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0,
        },
    });
    if mesh.blend {
        material["alphaMode"] = json!("BLEND");
    }
    let maps = mesh.maps;
    if let Some(index) = maps.color {
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": texture(index) });
    }
    if let Some(index) = maps.normal {
        material["normalTexture"] = json!({ "index": texture(index) });
    }
    // The factors multiply the texture, so 1 leaves it as authored.
    if let Some(index) = maps.metallic_roughness {
        let pbr = &mut material["pbrMetallicRoughness"];
        pbr["metallicRoughnessTexture"] = json!({ "index": texture(index) });
        pbr["metallicFactor"] = json!(1.0);
        pbr["roughnessFactor"] = json!(1.0);
    }
    finish(&mut material, mesh, texture, used);
    material
}

fn bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
    positions.as_chunks::<3>().0.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(min, max), p| {
            (
                [min[0].min(p[0]), min[1].min(p[1]), min[2].min(p[2])],
                [max[0].max(p[0]), max[1].max(p[1]), max[2].max(p[2])],
            )
        },
    )
}
