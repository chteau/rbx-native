//! What a downloaded asset becomes: the instances `insert_asset` plans,
//! read from its bytes.

use rbx_dom::{BrickColor, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::super::resolve;
use super::Kind;
use crate::properties::edit::commit;

/// The first name along `path` that names nothing, for Studio's warning.
pub(super) fn missing<'a>(dom: &WeakDom, path: &'a str) -> &'a str {
    let relative = path.strip_prefix("game.").unwrap_or(path);
    let segments: Vec<&str> = relative.split('.').collect();
    (1..segments.len())
        .rev()
        .find(|&known| resolve(dom, &segments[..known].join(".")).is_some())
        .map_or(segments[0], |known| segments[known])
}

/// The instances asset `asset_id` becomes, as roots, named `name`.
pub(in super::super) fn build(
    asset_id: u64,
    kind: Kind,
    bytes: &[u8],
    name: &str,
) -> Result<WeakDom, String> {
    let Some((class, property)) = kind.instance() else {
        let mut dom = read_model(bytes).map_err(|err| format!("asset {asset_id}: {err}"))?;
        let roots = dom.root_refs().to_vec();
        match (kind, roots.as_slice()) {
            (Kind::Animation, [only]) => {
                let _ = dom.set_name(*only, name);
            }
            _ => {
                let model = dom.new_instance("Model", name, None);
                for root in roots {
                    dom.set_parent(root, Some(model));
                }
            }
        }
        return Ok(dom);
    };
    let mut dom = WeakDom::new();
    let made = dom.new_instance(class, name, None);
    commit(
        &mut dom,
        ReflectionDatabase::shared(),
        made,
        property,
        &format!("rbxassetid://{asset_id}"),
    )?;
    if kind == Kind::Mesh {
        let size = mesh_size(bytes).map_err(|err| format!("asset {asset_id}: {err}"))?;
        for property in ["Size", "InitialSize"] {
            let _ = dom.set_property(made, property, Variant::Vector3(size));
        }
        let _ = dom.set_property(made, "Anchored", Variant::Bool(true));
    }
    Ok(dom)
}

pub(super) fn holds_package_link(dom: &WeakDom) -> bool {
    let mut stack = dom.root_refs().to_vec();
    while let Some(instance) = stack.pop().and_then(|r| dom.get(r)) {
        if instance.class() == "PackageLink" {
            return true;
        }
        stack.extend_from_slice(instance.children());
    }
    false
}

/// A model file as a loaded place holds it: properties under the names
/// Studio saves, and no part colour kept as a `BrickColor`.
pub(super) fn read_model(bytes: &[u8]) -> Result<WeakDom, String> {
    let mut dom = rbx_viewer::parse_place(bytes, std::path::Path::new("the asset"))?;
    settle_brick_colors(&mut dom);
    Ok(dom)
}

/// An old model carries a part's colour as `BrickColor`, which Studio loads
/// into the part's colour and never saves; left as it is, a binary place
/// would hold it on these parts only, which the format cannot express.
fn settle_brick_colors(dom: &mut WeakDom) {
    let database = ReflectionDatabase::shared();
    let mut stack = dom.root_refs().to_vec();
    while let Some(instance) = stack.pop().and_then(|r| dom.get_mut(r)) {
        stack.extend_from_slice(instance.children());
        let saved = database
            .resolve_property(instance.class(), "BrickColor")
            .is_none_or(|property| property.can_save);
        if saved {
            continue;
        }
        let properties = instance.properties_mut();
        let Some(value) = properties.remove("BrickColor") else {
            continue;
        };
        let number = match value {
            Variant::BrickColor(number) => Some(number),
            Variant::Int32(number) => u32::try_from(number).ok(),
            _ => None,
        };
        if let Some(color) = number.and_then(BrickColor::from_number) {
            let [r, g, b] = color.rgb;
            properties
                .entry("Color3uint8".to_owned())
                .or_insert(Variant::Color3uint8 { r, g, b });
        }
    }
}

/// A `MeshPart`'s natural size is its mesh's extent.
fn mesh_size(bytes: &[u8]) -> Result<Vector3Data, String> {
    let mesh = rbx_mesh::parse(bytes).map_err(|err| err.to_string())?;
    if mesh.vertices.is_empty() {
        return Err("the mesh has no vertices".to_owned());
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex.position[axis]);
            max[axis] = max[axis].max(vertex.position[axis]);
        }
    }
    Ok(Vector3Data {
        x: max[0] - min[0],
        y: max[1] - min[1],
        z: max[2] - min[2],
    })
}
