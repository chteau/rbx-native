//! Binary format serialization: the encode counterpart of the whole `deserialize` pipeline.
//!
//! Entry point: [`serialize`]. Builds a class/referent layout from the DOM, then writes
//! the header and chunks the reader expects: META, SSTR, one INST+PROP group per class,
//! PRNT, END.

mod chunk;
mod inst;
mod plan;
mod prnt;
mod prop;
mod service;
mod sstr;
mod writer;

use std::collections::BTreeSet;

use rbx_dom::{Variant, WeakDom};
use thiserror::Error;

use plan::Plan;
use sstr::SharedStringTable;
use writer::Writer;

const MAGIC: &[u8; 8] = b"<roblox!";
const SIGNATURE: [u8; 6] = [0x89, 0xFF, 0x0D, 0x0A, 0x1A, 0x0A];

// The DOM stores the instance name in a dedicated field rather than the property
// map; the wire format has no such distinction, so it is written back as an
// ordinary String property on every class, same as `deserializer` reads it.
const NAME_PROPERTY: &str = "Name";

/// Errors that can occur while turning a DOM into a binary file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SerializeError {
    /// No encoder is registered for this `Variant` kind or wire type id.
    #[error("no binary encoder for the `{0}` property kind")]
    Unsupported(&'static str),

    /// A property has a value on some instances of a class but not others, which the
    /// binary format can only express for `Ref` properties (via the null referent).
    #[error(
        "{class}.{property} is missing a value on some instances of the class but not \
         others, which the binary format cannot express for this property type"
    )]
    InconsistentProperty { class: String, property: String },

    /// Instances of the same class disagree on the `Variant` shape of one property.
    #[error("{class}.{property} mixes incompatible value shapes across instances")]
    TypeMismatch { class: String, property: String },

    /// `Variant::Unknown` blobs of different byte lengths across a class's instances:
    /// the reader's generic fallback can only split a payload into equal-size slices.
    #[error(
        "{class}.{property} carries Unknown blobs of different byte lengths, which the \
         reader's generic fallback cannot losslessly split back apart"
    )]
    VariableLengthUnknown { class: String, property: String },
}

impl SerializeError {
    pub(crate) fn missing(class: &str, property: &str) -> Self {
        SerializeError::InconsistentProperty {
            class: class.to_owned(),
            property: property.to_owned(),
        }
    }

    pub(crate) fn mismatch(class: &str, property: &str) -> Self {
        SerializeError::TypeMismatch {
            class: class.to_owned(),
            property: property.to_owned(),
        }
    }
}

/// Serializes a DOM tree into a Roblox binary file (.rbxm or .rbxl layout).
///
/// The output is a fresh encoding, not a byte-for-byte replica of any file the DOM
/// might have been read from: referents are reused from the DOM's own `Ref` values,
/// but class ids, chunk order and property order are assigned deterministically here.
/// `deserialize(&serialize(&dom)?)` reconstructs an equivalent tree.
///
/// A property some instances of a class hold and others don't is written
/// with a neutral value for the others — see [`serialize_with_defaults`],
/// which a caller that knows each class's defaults should use instead.
pub fn serialize(dom: &WeakDom) -> Result<Vec<u8>, SerializeError> {
    serialize_with_defaults(dom, |_, _| None)
}

/// [`serialize`], with `default(class, property)` answering what an
/// instance of `class` that holds no `property` has: the binary format
/// stores one value per instance for every property any instance of the
/// class holds, and whatever fills the gap is what Studio loads. The class
/// default is the only honest fill — a zero turns a pasted part's
/// `archivable` column into `false` for every other part, and Studio drops
/// non-archivable instances from its next save. A property the lookup
/// cannot answer for falls back to the neutral value.
pub fn serialize_with_defaults(
    dom: &WeakDom,
    default: impl Fn(&str, &str) -> Option<Variant>,
) -> Result<Vec<u8>, SerializeError> {
    let plan = plan::build(dom);
    // Built in a pass of its own, before any PROP chunk is written: the table is
    // deduplicated across the *whole file*, so every group's content must be seen
    // before the first index referencing it can be assigned.
    let table = build_shared_string_table(dom, &plan, &default);

    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(&file_header(&plan));
    out.extend(chunk::write_chunk(b"META", &meta_payload()));
    out.extend(chunk::write_chunk(b"SSTR", &table.write()));

    for class in &plan.classes {
        out.extend(chunk::write_chunk(b"INST", &inst::write(class)));

        for name in property_names(dom, class) {
            let values = collect_values(dom, class, &name, &default);
            let (type_id, payload) = prop::encode(&class.class_name, &name, &values, &table)?;
            out.extend(chunk::write_chunk(
                b"PROP",
                &prop_header(class.class_id, &name, type_id, &payload),
            ));
        }
    }

    out.extend(chunk::write_chunk(b"PRNT", &prnt::write(&plan)));
    out.extend(chunk::write_chunk(b"END\0", &[]));

    Ok(out)
}

fn build_shared_string_table(
    dom: &WeakDom,
    plan: &Plan,
    default: &impl Fn(&str, &str) -> Option<Variant>,
) -> SharedStringTable {
    let mut table = SharedStringTable::new();
    for class in &plan.classes {
        for name in property_names(dom, class) {
            let values = collect_values(dom, class, &name, default);
            sstr::collect_group(&mut table, &values);
        }
    }
    table
}

fn file_header(plan: &Plan) -> [u8; 32] {
    let mut header = [0u8; 32];
    header[0..8].copy_from_slice(MAGIC);
    header[8..14].copy_from_slice(&SIGNATURE);
    header[14..16].copy_from_slice(&0u16.to_le_bytes());
    header[16..20].copy_from_slice(&(plan.classes.len() as i32).to_le_bytes());
    header[20..24].copy_from_slice(&(plan.order.len() as i32).to_le_bytes());
    header
}

// No metadata flags are round-tripped by this crate (deserializer never reads META),
// so an empty table is always valid and keeps the chunk purely structural.
fn meta_payload() -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(0);
    writer.into_bytes()
}

fn prop_header(class_id: i32, name: &str, type_id: u8, payload: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.i32(class_id);
    writer.sized_name(name);
    writer.u8(type_id);
    writer.bytes(payload);
    writer.into_bytes()
}

// Alphabetical union of every property key set on any instance of `class`, plus the
// synthetic "Name" property every class carries.
fn property_names(dom: &WeakDom, class: &plan::ClassPlan) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = class
        .referents
        .iter()
        .filter_map(|&referent| dom.get(referent))
        .flat_map(|instance| instance.properties().keys().cloned())
        .collect();
    names.insert(NAME_PROPERTY.to_owned());
    names
}

fn collect_values(
    dom: &WeakDom,
    class: &plan::ClassPlan,
    name: &str,
    default: &impl Fn(&str, &str) -> Option<Variant>,
) -> Vec<Option<Variant>> {
    let mut values: Vec<Option<Variant>> = class
        .referents
        .iter()
        .map(|&referent| {
            let instance = dom.get(referent)?;
            if name == NAME_PROPERTY {
                Some(Variant::String(instance.name().to_owned()))
            } else {
                instance.properties().get(name).cloned()
            }
        })
        .collect();
    if values.iter().any(Option::is_none) {
        fill_missing(&mut values, default(&class.class_name, name));
    }
    values
}

// A script that sets only some properties on a freshly created instance (e.g.
// `Instance.new("Part", workspace)`) leaves the rest absent, while a file-loaded
// instance of the same class carries all of them; the binary format needs one
// value per instance in a PROP chunk regardless. The gap is filled with
// `default`, the class default, where it has the column's own type, and
// otherwise with the type's neutral value. Kinds with neither (sequences,
// `Font`, `Unknown` blobs, and anything else not matched below) are left as
// `None`, which `prop::encode`'s `map_dense` still rejects with
// `InconsistentProperty`.
//
// `Ref` is deliberately not filled here even though it has a "null" value: the
// existing per-instance encoder already writes `None` as the null referent
// (-1) without erroring, so there is nothing missing to fix for that kind.
fn fill_missing(values: &mut [Option<Variant>], default: Option<Variant>) {
    let Some(sample) = values.iter().flatten().next() else {
        return;
    };
    let same_type =
        |value: &Variant| std::mem::discriminant(value) == std::mem::discriminant(sample);
    let Some(neutral) = default.filter(same_type).or_else(|| neutral_for(sample)) else {
        return;
    };
    for value in values.iter_mut() {
        if value.is_none() {
            *value = Some(neutral.clone());
        }
    }
}

/// The type-appropriate "nothing set" value for a property, used to fill
/// instances that never had it assigned. `None` means the kind has no obvious
/// neutral value, so a missing instance is left missing (and still errors).
fn neutral_for(sample: &Variant) -> Option<Variant> {
    use rbx_dom::{CFrameData, Color3Data, Content, UDim, UDim2, UniqueId, Vector3Data};

    match sample {
        Variant::Bool(_) => Some(Variant::Bool(false)),
        Variant::Int32(_) => Some(Variant::Int32(0)),
        Variant::Int64(_) => Some(Variant::Int64(0)),
        Variant::Float32(_) => Some(Variant::Float32(0.0)),
        Variant::Float64(_) => Some(Variant::Float64(0.0)),
        Variant::String(_) => Some(Variant::String(String::new())),
        Variant::Vector3(_) => Some(Variant::Vector3(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        })),
        Variant::Color3(_) => Some(Variant::Color3(Color3Data {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        })),
        // Byte-packed sibling of `Color3`: same neutral, black.
        Variant::Color3uint8 { .. } => Some(Variant::Color3uint8 { r: 0, g: 0, b: 0 }),
        Variant::CFrame(_) => Some(Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })),
        Variant::UDim(_) => Some(Variant::UDim(UDim {
            scale: 0.0,
            offset: 0,
        })),
        Variant::UDim2(_) => Some(Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.0,
                offset: 0,
            },
            y: UDim {
                scale: 0.0,
                offset: 0,
            },
        })),
        Variant::Enum(_) => Some(Variant::Enum(0)),
        Variant::Content(_) => Some(Variant::Content(Content::None)),
        // A bitset of engine capabilities, same shape as an enum ordinal: 0 means
        // none granted, which is what an instance with no value implies anyway.
        Variant::SecurityCapabilities(_) => Some(Variant::SecurityCapabilities(0)),
        // `Default` is exactly what "no custom physical properties were ever set"
        // means for a fresh instance.
        Variant::PhysicalProperties(_) => Some(Variant::PhysicalProperties(
            rbx_dom::PhysicalProperties::Default,
        )),
        // All-zero is what the format itself already uses for an unset id: the
        // real fixture's `HistoryId` (a `UniqueId`) is all-zero on every instance
        // that never went through a save session (see
        // `test_place_history_ids_are_all_blank` in `rbx_binary`'s integration
        // tests), so it is not a value this crate invents.
        Variant::UniqueId(_) => Some(Variant::UniqueId(UniqueId {
            index: 0,
            time: 0,
            random: 0,
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deserialize;
    use rbx_dom::Ref;

    #[test]
    fn a_value_one_instance_lacks_is_filled_with_the_given_default() {
        let mut dom = WeakDom::new();
        let stored = dom.new_instance("Part", "Stored", None);
        let bare = dom.new_instance("Part", "Bare", None);
        dom.set_property(stored, "CanCollide", Variant::Bool(false))
            .unwrap();
        // A default of the wrong type is no default at all.
        dom.set_property(stored, "Transparency", Variant::Float32(0.5))
            .unwrap();

        let bytes = serialize_with_defaults(&dom, |class, property| match (class, property) {
            ("Part", "CanCollide") => Some(Variant::Bool(true)),
            ("Part", "Transparency") => Some(Variant::String("nope".into())),
            _ => None,
        })
        .unwrap();
        let reloaded = deserialize(&bytes).unwrap();

        let bare = reloaded.get(bare).unwrap().properties();
        assert_eq!(bare.get("CanCollide"), Some(&Variant::Bool(true)));
        assert_eq!(bare.get("Transparency"), Some(&Variant::Float32(0.0)));
    }

    #[test]
    fn header_round_trips_through_the_parser() {
        let mut dom = WeakDom::new();
        dom.new_instance("Folder", "A", None);
        dom.new_instance("Part", "B", None);

        let bytes = serialize(&dom).unwrap();
        let (header, _) = crate::parse_header(&bytes).unwrap();

        assert_eq!(header.num_types, 2);
        assert_eq!(header.num_instances, 2);
    }

    #[test]
    fn an_empty_dom_serializes_and_deserializes_to_an_empty_dom() {
        let dom = WeakDom::new();
        let bytes = serialize(&dom).unwrap();
        let round_tripped = deserialize(&bytes).unwrap();

        assert!(round_tripped.root_refs().is_empty());
    }

    #[test]
    fn a_single_instance_round_trips_its_name_and_class() {
        let mut dom = WeakDom::new();
        let referent = dom.new_instance("Part", "MyPart", None);

        let bytes = serialize(&dom).unwrap();
        let round_tripped = deserialize(&bytes).unwrap();

        let instance = round_tripped.get(Ref::new(referent.value())).unwrap();
        assert_eq!(instance.class(), "Part");
        assert_eq!(instance.name(), "MyPart");
    }

    #[test]
    fn a_property_missing_on_some_instances_is_filled_with_its_neutral_value() {
        // Simulates a file-loaded Part (several properties set) alongside a bare
        // `Instance.new("Part", workspace)` (none set): the binary format needs a
        // value on every instance, so the missing ones must be filled rather than
        // rejected.
        let mut dom = WeakDom::new();
        let loaded = dom.new_instance("Part", "Loaded", None);
        dom.set_property(loaded, "Transparency", Variant::Float32(0.5))
            .unwrap();
        dom.set_property(loaded, "Anchored", Variant::Bool(true))
            .unwrap();
        let bare = dom.new_instance("Part", "Bare", None);

        let bytes = serialize(&dom).unwrap();
        let round_tripped = deserialize(&bytes).unwrap();

        let bare = round_tripped
            .get(Ref::new(bare.value()))
            .expect("the bare instance round-trips");
        assert_eq!(
            bare.properties().get("Transparency"),
            Some(&Variant::Float32(0.0))
        );
        assert_eq!(
            bare.properties().get("Anchored"),
            Some(&Variant::Bool(false))
        );
    }

    #[test]
    fn a_kind_with_no_neutral_value_is_still_rejected_when_missing() {
        use rbx_dom::NumberSequence;

        let mut dom = WeakDom::new();
        let a = dom.new_instance("ParticleEmitter", "A", None);
        let _b = dom.new_instance("ParticleEmitter", "B", None);
        dom.set_property(
            a,
            "Transparency",
            Variant::NumberSequence(NumberSequence { keypoints: vec![] }),
        )
        .unwrap();

        let err = serialize(&dom).unwrap_err();
        assert!(matches!(err, SerializeError::InconsistentProperty { .. }));
    }
}
