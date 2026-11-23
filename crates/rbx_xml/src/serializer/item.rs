use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use md5::{Digest, Md5};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::value;
use super::writer::Writer;
use crate::attr_refs;
use crate::error::XmlError;

/// The binary type id of Terrain's `VoxelGridAssetContentMap`.
const CONTENT_MAP_TYPE_ID: u8 = 0x23;

/// Properties Studio types as NetworkAssetRef (the same list as `rbx_binary`'s
/// writer): a SharedString it only accepts as a `<NetAssetRef>` element. A
/// union's `SolidMeshHolder` holds its whole CSG geometry.
const NET_ASSET_REF_PROPERTIES: &[&str] = &["SolidMeshHolder"];

pub(super) fn write_item<'a>(
    writer: &mut Writer,
    dom: &'a WeakDom,
    this: Ref,
    shared: &mut BTreeMap<String, &'a [u8]>,
) -> Result<(), XmlError> {
    // `this` always comes from `dom`'s own `root_refs`/`children`, so it is
    // always present; a missing entry would mean `WeakDom`'s own tree invariant
    // (every referent in `children`/`root_refs` has a matching instance) broke.
    let instance = dom
        .get(this)
        .expect("WeakDom tree invariant: referent listed as a child/root must exist");

    writer.open(
        "Item",
        &[("class", instance.class()), ("referent", &referent(this))],
    );

    writer.open("Properties", &[]);
    // The DOM keeps the instance name in its own field rather than the property
    // map (see `deserializer`'s `NAME_PROPERTY` redirect), so it is written back
    // as an ordinary `string` property here to match.
    value::string_property(writer, instance.name());
    let db = ReflectionDatabase::shared();
    for (name, prop_value) in instance.properties() {
        let saved = db.saved_type(instance.class(), name);
        if let Some((blob, refs)) = attr_refs::split(name, prop_value) {
            value::encode(writer, name, &blob, saved)?;
            for (name, target) in refs {
                value::encode(writer, &name, &Variant::Ref(target), None)?;
            }
            continue;
        }
        let net_asset_ref = NET_ASSET_REF_PROPERTIES.contains(&name.as_str());
        match prop_value {
            Variant::Unknown { type_id: 0x1C, raw } => {
                let key = STANDARD.encode(Md5::digest(raw));
                let tag = if net_asset_ref {
                    "NetAssetRef"
                } else {
                    "SharedString"
                };
                writer.leaf(tag, &[("name", name)], &key);
                shared.insert(key, raw);
            }
            // The legacy always-empty String shape of a NetAssetRef property,
            // which Studio refuses to convert; its own save has none.
            _ if net_asset_ref => {}
            // Terrain's `VoxelGridAssetContentMap`, which every place Studio
            // 0.744 saves carries, is a binary-only kind with no XML tag we
            // know, and refusing it would fail every such save. Studio opens
            // the place without it and writes it back itself. Any other
            // unknown kind still refuses (see `value::encode`).
            Variant::Unknown {
                type_id: CONTENT_MAP_TYPE_ID,
                ..
            } => {}
            _ => value::encode(writer, name, prop_value, saved)?,
        }
    }
    writer.close("Properties");

    for &child in instance.children() {
        write_item(writer, dom, child, shared)?;
    }

    writer.close("Item");
    Ok(())
}

/// Formats a `Ref` as the referent string this serializer's `Item`s and `Ref`/
/// `Content` properties use; shared with `serializer::value` so both sides agree.
pub(crate) fn referent(r: Ref) -> String {
    format!("RBX{}", r.value())
}
