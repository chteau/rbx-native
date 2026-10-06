//! XML place/model format writing: the encode counterpart of `deserialize`.
//!
//! Entry point: [`serialize`]. Walks the DOM tree once, writing `Item`s as it
//! goes; each property is delegated to `serializer::value`, mirroring the
//! reader's own `value` module one `Variant` kind at a time.

mod value;
mod writer;

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use md5::{Digest, Md5};
use rbx_dom::{Ref, Variant, WeakDom};

use crate::error::XmlError;
use writer::Writer;

// Fixed rather than derived from the DOM: this crate has no reflection database
// to tell whether a given tree actually has (or needs) explicit joint instances,
// and Roblox's own Studio always writes this Meta tag with this value.
const EXPLICIT_AUTO_JOINTS: (&str, &str) = ("ExplicitAutoJoints", "true");

/// Serializes a DOM tree into a Roblox XML place/model file (`.rbxlx`/`.rbxmx`).
///
/// The output is a fresh encoding, not a byte-for-byte replica of any file the DOM
/// might have been read from: referents are derived from the DOM's own `Ref`
/// values (`RBX` + the numeric id) rather than reusing whatever string a source
/// file used. `deserialize(&serialize(&dom)?)` reconstructs an equivalent tree,
/// though not necessarily with the same numeric `Ref` values (the reader assigns
/// its own referents in file order on every parse; see its `assign_referents`).
///
/// A SharedString (`Variant::Unknown { 0x1C }`, how both readers hand one
/// back) is written as a `SharedString` reference into a `SharedStrings`
/// table after the last `Item`, keyed by its MD5 like Studio's own files.
pub fn serialize(dom: &WeakDom) -> Result<String, XmlError> {
    let mut writer = Writer::new();

    writer.open("roblox", &[("version", "4")]);
    writer.leaf(
        "Meta",
        &[("name", EXPLICIT_AUTO_JOINTS.0)],
        EXPLICIT_AUTO_JOINTS.1,
    );
    let mut shared = BTreeMap::new();
    for &root in dom.root_refs() {
        write_item(&mut writer, dom, root, &mut shared)?;
    }
    if !shared.is_empty() {
        writer.open("SharedStrings", &[]);
        for (key, raw) in &shared {
            writer.leaf("SharedString", &[("md5", key)], &STANDARD.encode(raw));
        }
        writer.close("SharedStrings");
    }
    writer.close("roblox");

    Ok(writer.into_string())
}

fn write_item<'a>(
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
    for (name, prop_value) in instance.properties() {
        match prop_value {
            Variant::Unknown { type_id: 0x1C, raw } => {
                let key = STANDARD.encode(Md5::digest(raw));
                writer.leaf("SharedString", &[("name", name)], &key);
                shared.insert(key, raw);
            }
            _ => value::encode(writer, name, prop_value)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::{Content, Instance, Variant, Vector3Data};

    #[test]
    fn empty_dom_serializes_to_just_the_root_and_meta() {
        let dom = WeakDom::new();
        let xml = serialize(&dom).unwrap();
        assert_eq!(
            xml,
            "<roblox version=\"4\">\n  <Meta name=\"ExplicitAutoJoints\">true</Meta>\n</roblox>\n"
        );
    }

    #[test]
    fn single_instance_round_trips_class_name_and_a_property() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Baseplate", None);
        dom.set_property(
            part,
            "size",
            Variant::Vector3(Vector3Data {
                x: 512.0,
                y: 1.2,
                z: 512.0,
            }),
        )
        .unwrap();

        let xml = serialize(&dom).unwrap();
        let round_tripped = crate::deserialize(&xml).unwrap();

        let roots = round_tripped.root_refs();
        assert_eq!(roots.len(), 1);
        let got = round_tripped.get(roots[0]).unwrap();
        assert_eq!(got.class(), "Part");
        assert_eq!(got.name(), "Baseplate");
        assert_eq!(
            got.properties().get("size"),
            Some(&Variant::Vector3(Vector3Data {
                x: 512.0,
                y: 1.2,
                z: 512.0
            }))
        );
    }

    #[test]
    fn nested_children_and_a_ref_property_round_trip() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part_a = dom.new_instance("Part", "A", Some(workspace));
        let part_b = dom.new_instance("Part", "B", Some(workspace));
        dom.set_property(part_a, "Target", Variant::Ref(part_b))
            .unwrap();
        dom.set_property(part_a, "Source", Variant::Content(Content::Object(part_b)))
            .unwrap();

        let xml = serialize(&dom).unwrap();
        let round_tripped = crate::deserialize(&xml).unwrap();

        let ws = round_tripped.get(round_tripped.root_refs()[0]).unwrap();
        assert_eq!(ws.children().len(), 2);
        let a = round_tripped
            .get(ws.children()[0])
            .filter(|i| i.name() == "A")
            .or_else(|| round_tripped.get(ws.children()[1]))
            .unwrap();
        let target_ref = match a.properties().get("Target") {
            Some(Variant::Ref(r)) => *r,
            other => panic!("expected a Ref property, got {other:?}"),
        };
        let target = round_tripped.get(target_ref).unwrap();
        assert_eq!(target.name(), "B");
        assert_eq!(
            a.properties().get("Source"),
            Some(&Variant::Content(Content::Object(target_ref)))
        );
    }

    #[test]
    fn a_texture_stored_as_string_and_as_content_both_round_trip() {
        // XML writes each instance's value on its own, so the two shapes a
        // Decal.Texture can hold never have to agree the way a binary column does.
        let mut dom = WeakDom::new();
        let loaded = dom.new_instance("Decal", "Loaded", None);
        let set = dom.new_instance("Decal", "Set", None);
        let string = Variant::String("rbxassetid://1".into());
        let content = Variant::Content(Content::Uri("rbxassetid://2".into()));
        dom.set_property(loaded, "Texture", string.clone()).unwrap();
        dom.set_property(set, "Texture", content.clone()).unwrap();

        let reloaded = crate::deserialize(&serialize(&dom).unwrap()).unwrap();

        let mut textures: Vec<_> = reloaded
            .root_refs()
            .iter()
            .map(|&r| {
                reloaded
                    .get(r)
                    .unwrap()
                    .properties()
                    .get("Texture")
                    .cloned()
            })
            .collect();
        textures.sort_by_key(|v| format!("{v:?}"));
        assert_eq!(textures, vec![Some(content), Some(string)]);
    }

    #[test]
    fn shared_strings_round_trip_through_one_deduplicated_table() {
        let mut dom = WeakDom::new();
        let mesh = Variant::Unknown {
            type_id: 0x1C,
            raw: vec![0xFF, 1, 2],
        };
        let a = dom.new_instance("Model", "A", None);
        let b = dom.new_instance("Model", "B", None);
        dom.set_property(a, "ModelMeshData", mesh.clone()).unwrap();
        dom.set_property(b, "ModelMeshData", mesh.clone()).unwrap();

        let xml = serialize(&dom).unwrap();
        assert_eq!(xml.matches("<SharedString md5=").count(), 1);

        let back = crate::deserialize(&xml).unwrap();
        for &root in back.root_refs() {
            let got = back.get(root).unwrap().properties().get("ModelMeshData");
            assert_eq!(got, Some(&mesh));
        }
    }

    #[test]
    fn non_utf8_unknown_blob_round_trips_through_binary_string() {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Terrain", "Terrain"));
        let raw = vec![0xFF, 0xFE, 0x00, 0x80];
        dom.set_property(
            Ref::new(1),
            "SmoothGrid",
            Variant::Unknown {
                type_id: 1,
                raw: raw.clone(),
            },
        )
        .unwrap();

        let xml = serialize(&dom).unwrap();
        let round_tripped = crate::deserialize(&xml).unwrap();
        let got = round_tripped.get(round_tripped.root_refs()[0]).unwrap();
        assert_eq!(
            got.properties().get("SmoothGrid"),
            Some(&Variant::Unknown { type_id: 1, raw })
        );
    }

    #[test]
    fn instances_of_the_same_class_may_disagree_on_which_properties_are_set() {
        // Unlike `rbx_binary` (one PROP chunk per property, shared across every
        // instance of a class), XML writes each `Item`'s properties independently,
        // so a property present on one instance and absent on another of the same
        // class is not a format-level inconsistency here.
        let mut dom = WeakDom::new();
        let loaded = dom.new_instance("Part", "Loaded", None);
        dom.set_property(loaded, "Transparency", Variant::Float32(0.5))
            .unwrap();
        dom.new_instance("Part", "Bare", None);

        let xml = serialize(&dom).unwrap();
        let round_tripped = crate::deserialize(&xml).unwrap();

        let bare = round_tripped
            .root_refs()
            .iter()
            .find_map(|&r| round_tripped.get(r).filter(|i| i.name() == "Bare"))
            .expect("the bare instance round-trips");
        assert_eq!(bare.class(), "Part");
        assert!(
            bare.properties().get("Transparency").is_none(),
            "XML has no need to invent a value for a property the instance never had"
        );
    }

    #[test]
    fn unresolved_shared_string_index_is_reported_as_unsupported() {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Script", "A"));
        dom.set_property(Ref::new(1), "Source", Variant::SharedString(4))
            .unwrap();

        assert!(matches!(
            serialize(&dom),
            Err(XmlError::Unsupported("SharedString"))
        ));
    }
}
