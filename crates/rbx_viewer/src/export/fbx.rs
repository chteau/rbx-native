//! Binary FBX 7.4, one self-contained file: the instance tree as models
//! placed relative to their parents, each part's surfaces as one geometry
//! (see [`geometry`]), a Phong material per surface, and the PNGs those
//! materials use embedded in the file, colour maps with the part's colour
//! baked in and a one-texel swatch on plain surfaces (see [`diffuse`]).
//!
//! Units are studs at one stud to the metre (`UnitScaleFactor` 100, FBX
//! counting in centimetres), in Roblox's own right-handed Y-up frame, so a
//! part comes out the size the glTF export makes it. Colours are linear,
//! as Blender's FBX exporter writes them and its importer reads them back.
//!
//! What FBX's Phong material has no slot every reader agrees on is left
//! out: metalness and roughness maps, a ForceField's pattern and Glass's
//! refraction (Glass keeps its transparency). Neon is emissive in its own
//! colour.

mod diffuse;
mod geometry;
mod tree;

use std::collections::BTreeMap;

use glam::{EulerRot, Mat4};

use super::gltf::node_worlds;
use super::{Export, ExportMesh, Finish};
use tree::{
    name_class, p_color, p_double, p_enum, p_int, p_local, p_number, p_string, p_url, p_user,
    properties, Node, Prop, CREATION_TIME, FILE_ID,
};

/// What every object id is offset by: 0 is the scene's root.
const FIRST_ID: i64 = 1_000_000;

/// The whole export as one `.fbx` file.
pub fn fbx(export: &Export) -> Result<Vec<u8>, String> {
    let mut ids = FIRST_ID..;
    let mut objects = Node::new("Objects", Vec::new());
    let mut connections = Node::new("Connections", Vec::new());
    let mut connect = |child: i64, parent: i64, property: Option<&str>| {
        let mut props: Vec<Prop> = vec![
            if property.is_some() { "OP" } else { "OO" }.into(),
            child.into(),
            parent.into(),
        ];
        props.extend(property.map(Prop::from));
        connections.push(Node::new("C", props));
    };

    let materials: Vec<i64> = export
        .meshes
        .iter()
        .map(|_| ids.next().unwrap_or_default())
        .collect();
    // Each image once, as a texture over an embedded clip: a normal map by
    // its index in `Export::textures`, a colour map by that and the tint
    // baked into it (see [`diffuse`]), and a surface with no colour map by
    // its colour, as a one-texel swatch.
    let mut textures = BTreeMap::new();
    for (mesh, &id) in export.meshes.iter().zip(&materials) {
        objects.push(material(id, mesh));
        let maps = [
            (
                Some(mesh.maps.color.map_or_else(
                    || Map::Swatch(diffuse::swatch_texel(mesh.color)),
                    |i| Map::Color(diffuse::Key::of(mesh, i)),
                )),
                "DiffuseColor",
            ),
            (mesh.maps.normal.map(Map::Normal), "NormalMap"),
        ];
        for (map, slot) in maps {
            let Some(map) = map else { continue };
            let count = textures.len();
            let texture = *textures.entry(map).or_insert_with(|| {
                let (texture, video) = (
                    ids.next().unwrap_or_default(),
                    ids.next().unwrap_or_default(),
                );
                let png = match map {
                    Map::Color(key) => diffuse::bake(&export.textures[key.index()], key),
                    Map::Normal(index) => export.textures[index].clone(),
                    Map::Swatch(texel) => diffuse::swatch(texel),
                };
                objects.push(texture_node(texture, count));
                objects.push(video_node(video, count, png));
                connect(video, texture, None);
                texture
            });
            connect(texture, id, Some(slot));
        }
    }

    let worlds = node_worlds(export);
    let models: Vec<i64> = export
        .nodes
        .iter()
        .map(|_| ids.next().unwrap_or_default())
        .collect();
    let mut geometries = 0;
    for (index, node) in export.nodes.iter().enumerate() {
        let id = models[index];
        let parent = node.parent.map_or(Mat4::IDENTITY, |p| worlds[p]);
        let local = parent.inverse() * worlds[index];
        let mut user = vec![p_user("RobloxInstanceType", &node.class)];
        user.extend(
            node.material
                .as_deref()
                .map(|material| p_user("Material", material)),
        );
        objects.push(model(id, &node.name, !node.meshes.is_empty(), local, user));
        connect(id, node.parent.map_or(0, |p| models[p]), None);
        if node.meshes.is_empty() {
            continue;
        }
        let surfaces: Vec<&ExportMesh> = node.meshes.iter().map(|&m| &export.meshes[m]).collect();
        let geometry_id = ids.next().unwrap_or_default();
        objects.push(geometry::geometry(
            geometry_id,
            &node.name,
            &surfaces,
            worlds[index].inverse(),
        ));
        geometries += 1;
        connect(geometry_id, id, None);
        for &mesh in &node.meshes {
            connect(materials[mesh], id, None);
        }
    }

    let counts = [
        ("GlobalSettings", 1),
        ("Model", export.nodes.len()),
        ("Geometry", geometries),
        ("Material", materials.len()),
        ("Texture", textures.len()),
        ("Video", textures.len()),
    ];
    tree::file(&[
        header(),
        Node::new("FileId", vec![Prop::Bytes(FILE_ID.to_vec())]),
        Node::value("CreationTime", CREATION_TIME),
        Node::value("Creator", CREATOR),
        global_settings(),
        Node::new("Documents", Vec::new())
            .with(Node::value("Count", 1))
            .with(
                Node::new(
                    "Document",
                    vec![
                        ids.next().unwrap_or_default().into(),
                        "Scene".into(),
                        "Scene".into(),
                    ],
                )
                .with(properties(vec![p_string("ActiveAnimStackName", "")]))
                .with(Node::value("RootNode", 0i64)),
            ),
        Node::new("References", Vec::new()),
        definitions(&counts),
        objects,
        connections,
    ])
}

const CREATOR: &str = "rbx-native";

/// An image a material uses, as it goes in the file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Map {
    Color(diffuse::Key),
    Normal(usize),
    /// Studio's 3D Importer leaves a material with no colour map grey,
    /// whatever its `DiffuseColor`, so plain surfaces get their colour as
    /// an image too.
    Swatch([u8; 3]),
}

fn header() -> Node {
    let mut stamp = Node::new("CreationTimeStamp", Vec::new()).with(Node::value("Version", 1000));
    for (field, value) in [
        ("Year", 1970),
        ("Month", 1),
        ("Day", 1),
        ("Hour", 10),
        ("Minute", 0),
        ("Second", 0),
        ("Millisecond", 0),
    ] {
        stamp.push(Node::value(field, value));
    }
    Node::new("FBXHeaderExtension", Vec::new())
        .with(Node::value("FBXHeaderVersion", 1003))
        .with(Node::value("FBXVersion", 7400))
        .with(Node::value("EncryptionType", 0))
        .with(stamp)
        .with(Node::value("Creator", CREATOR))
}

/// Y up, Z toward the viewer, X right: Roblox's frame, which FBX names the
/// way Maya's Y-up scenes are named.
fn global_settings() -> Node {
    Node::new("GlobalSettings", Vec::new())
        .with(Node::value("Version", 1000))
        .with(properties(vec![
            p_int("UpAxis", 1),
            p_int("UpAxisSign", 1),
            p_int("FrontAxis", 2),
            p_int("FrontAxisSign", 1),
            p_int("CoordAxis", 0),
            p_int("CoordAxisSign", 1),
            p_int("OriginalUpAxis", 1),
            p_int("OriginalUpAxisSign", 1),
            p_double("UnitScaleFactor", 100.0),
            p_double("OriginalUnitScaleFactor", 100.0),
        ]))
}

fn definitions(counts: &[(&str, usize)]) -> Node {
    let total: usize = counts.iter().map(|(_, count)| count).sum();
    let mut node = Node::new("Definitions", Vec::new())
        .with(Node::value("Version", 100))
        .with(Node::value("Count", total as i32));
    for &(kind, count) in counts.iter().filter(|(_, count)| *count > 0) {
        node.push(Node::value("ObjectType", kind).with(Node::value("Count", count as i32)));
    }
    node
}

/// A part (`Mesh`, with its geometry) or a container (`Null`), placed
/// relative to its parent. Rotation is Euler XYZ in degrees, FBX's default
/// order: X turns first.
fn model(id: i64, name: &str, mesh: bool, local: Mat4, user: Vec<Node>) -> Node {
    let (_, rotation, translation) = local.to_scale_rotation_translation();
    let (z, y, x) = rotation.normalize().to_euler(EulerRot::ZYX);
    let mut entries = vec![
        p_local("Lcl Translation", translation.to_array().map(f64::from)),
        p_local("Lcl Rotation", [x, y, z].map(|a| f64::from(a.to_degrees()))),
        p_local("Lcl Scaling", [1.0; 3]),
        p_enum("InheritType", 1),
    ];
    if mesh {
        entries.push(p_int("DefaultAttributeIndex", 0));
    }
    entries.extend(user);
    Node::new(
        "Model",
        vec![
            id.into(),
            name_class(name, "Model").into(),
            if mesh { "Mesh" } else { "Null" }.into(),
        ],
    )
    .with(Node::value("Version", 232))
    .with(properties(entries))
    .with(Node::value("MultiLayer", 0))
    .with(Node::value("MultiTake", 0))
    .with(Node::new("Shading", vec![Prop::Bool(true)]))
    .with(Node::value("Culling", "CullingOff"))
}

fn material(id: i64, mesh: &ExportMesh) -> Node {
    let [r, g, b, alpha] = mesh.color.map(f64::from);
    let neon = mesh.finish == Finish::Neon;
    let mut entries = vec![
        p_color("DiffuseColor", [r, g, b]),
        p_number("DiffuseFactor", 1.0),
        p_color("EmissiveColor", if neon { [r, g, b] } else { [0.0; 3] }),
        p_number("EmissiveFactor", if neon { 1.0 } else { 0.0 }),
        // Roblox's surfaces are matte unless their maps say otherwise.
        p_color("SpecularColor", [0.0; 3]),
        p_number("SpecularFactor", 0.0),
        p_number("Shininess", 2.0),
    ];
    if alpha < 1.0 || mesh.finish == Finish::ForceField {
        let opacity = if mesh.finish == Finish::ForceField {
            alpha.min(0.5)
        } else {
            alpha
        };
        entries.extend([
            p_color("TransparentColor", [1.0; 3]),
            p_number("TransparencyFactor", 1.0 - opacity),
            p_double("Opacity", opacity),
        ]);
    }
    Node::new(
        "Material",
        vec![
            id.into(),
            name_class(&mesh.name, "Material").into(),
            "".into(),
        ],
    )
    .with(Node::value("Version", 102))
    .with(Node::value("ShadingModel", "phong"))
    .with(Node::value("MultiLayer", 0))
    .with(properties(entries))
}

fn texture_file(index: usize) -> String {
    format!("texture_{index}.png")
}

fn texture_node(id: i64, index: usize) -> Node {
    let name = format!("texture_{index}");
    let file = texture_file(index);
    Node::new(
        "Texture",
        vec![id.into(), name_class(&name, "Texture").into(), "".into()],
    )
    .with(Node::value("Type", "TextureVideoClip"))
    .with(Node::value("Version", 202))
    .with(Node::value("TextureName", name_class(&name, "Texture")))
    .with(properties(vec![
        p_string("UVSet", "UVMap"),
        p_enum("WrapModeU", 0),
        p_enum("WrapModeV", 0),
    ]))
    .with(Node::value("Media", name_class(&name, "Video")))
    .with(Node::value("FileName", file.clone()))
    .with(Node::value("RelativeFilename", file))
}

/// The image itself, embedded as `Content` so the file stands alone.
fn video_node(id: i64, index: usize, png: Vec<u8>) -> Node {
    let name = format!("texture_{index}");
    let file = texture_file(index);
    Node::new(
        "Video",
        vec![id.into(), name_class(&name, "Video").into(), "Clip".into()],
    )
    .with(Node::value("Type", "Clip"))
    .with(properties(vec![p_url("Path", &file)]))
    .with(Node::value("UseMipMap", 0))
    .with(Node::value("Filename", file.clone()))
    .with(Node::value("RelativeFilename", file))
    .with(Node::new("Content", vec![Prop::Bytes(png)]))
}

#[cfg(test)]
mod tests;
