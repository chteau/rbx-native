//! Core deserialization logic that orchestrates binary file parsing into an in-memory DOM.
//!
//! The process reads chunks in any order, resolves references, and applies properties and
//! parent-child relationships to construct a tree of instances.

use std::collections::HashMap;

use rbx_dom::{Instance, Ref, Variant, WeakDom};

use crate::chunk::{read_chunks, Chunk};
use crate::chunks::{inst, prnt, prop, sstr};
use crate::error::BinaryError;
use crate::header::parse_header;

// The instance name lives in a dedicated field of the DOM rather than in the
// property map, so it is redirected instead of stored twice.
const NAME_PROPERTY: &str = "Name";

// Referents of one class, in the order INST declared them: PROP value arrays are
// indexed positionally against this list. `None` marks a referent we refused
// (negative id), which keeps the positions of the following ones correct.
type ClassReferents = HashMap<i32, Vec<Option<Ref>>>;

/// Deserializes a Roblox binary file (.rbxm or .rbxl) into a DOM tree.
///
/// The file is split into a header and chunks. Chunks are materialized up front because
/// nothing guarantees their order: PROP chunks may precede the SSTR table whose entries they index.
/// Instances are created first, then properties applied, then parent-child relationships established.
pub fn deserialize(bytes: &[u8]) -> Result<WeakDom, BinaryError> {
    deserialize_with_names(bytes).map(|(dom, _)| dom)
}

/// [`deserialize`], and every `(class, property)` pair the file holds values
/// for. The format stores a property once per class, not per instance, so
/// this is how a caller that treats properties by name learns what the
/// place holds without visiting every instance.
pub fn deserialize_with_names(
    bytes: &[u8],
) -> Result<(WeakDom, Vec<(String, String)>), BinaryError> {
    let (_header, body) = parse_header(bytes)?;
    // Chunks are materialized up front because nothing guarantees their order:
    // a PROP chunk may precede the SSTR table whose entries it indexes.
    let chunks: Vec<Chunk> = read_chunks(body).collect::<Result<_, _>>()?;

    let mut dom = WeakDom::new();
    let (classes, class_names) = insert_instances(&mut dom, &chunks)?;
    let shared = shared_strings(&chunks)?;

    let names = apply_properties(&mut dom, &chunks, &classes, &class_names, &shared);
    apply_parents(&mut dom, &chunks)?;

    Ok((dom, names))
}

fn chunks_named<'a>(chunks: &'a [Chunk], name: &str) -> impl Iterator<Item = &'a Chunk> + 'a {
    let name = name.to_owned();
    chunks.iter().filter(move |chunk| chunk.name_str() == name)
}

fn insert_instances(
    dom: &mut WeakDom,
    chunks: &[Chunk],
) -> Result<(ClassReferents, HashMap<i32, String>), BinaryError> {
    let mut classes = ClassReferents::new();
    let mut names = HashMap::new();

    for chunk in chunks_named(chunks, "INST") {
        let parsed = inst::parse(&chunk.data)?;
        let referents: Vec<Option<Ref>> = parsed
            .referents
            .iter()
            .map(|&referent| u32::try_from(referent).ok().map(Ref::new))
            .collect();

        for referent in referents.iter().flatten() {
            // Instances start out named after their class; the Name property
            // overwrites it below when the file carries one.
            dom.insert(Instance::new(
                *referent,
                parsed.class_name.as_str(),
                parsed.class_name.as_str(),
            ));
        }

        classes.insert(parsed.class_id, referents);
        names.insert(parsed.class_id, parsed.class_name);
    }

    Ok((classes, names))
}

fn shared_strings(chunks: &[Chunk]) -> Result<Vec<Vec<u8>>, BinaryError> {
    match chunks_named(chunks, "SSTR").next() {
        Some(chunk) => sstr::parse(&chunk.data),
        None => Ok(Vec::new()),
    }
}

// Property decoding is best-effort by design: a property whose class is unknown
// or whose payload we cannot walk is skipped or stored as `Variant::Unknown`,
// never turned into a hard failure of the whole file.
fn apply_properties(
    dom: &mut WeakDom,
    chunks: &[Chunk],
    classes: &ClassReferents,
    class_names: &HashMap<i32, String>,
    shared: &[Vec<u8>],
) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for chunk in chunks_named(chunks, "PROP") {
        let Ok(header) = prop::parse_header(&chunk.data) else {
            continue;
        };
        let Some(referents) = classes.get(&header.class_id) else {
            continue;
        };
        if let Some(class) = class_names.get(&header.class_id) {
            names.push((class.clone(), header.name.clone()));
        }

        let values = prop::decode(&header, referents.len(), shared);

        for (referent, value) in referents.iter().zip(values) {
            let (Some(referent), Some(value)) = (referent, value) else {
                continue;
            };
            let Some(instance) = dom.get_mut(*referent) else {
                continue;
            };

            match (header.name.as_str(), &value) {
                (NAME_PROPERTY, Variant::String(name)) => instance.set_name(name.clone()),
                _ => {
                    instance.properties_mut().insert(header.name.clone(), value);
                }
            }
        }
    }
    names
}

fn apply_parents(dom: &mut WeakDom, chunks: &[Chunk]) -> Result<(), BinaryError> {
    for chunk in chunks_named(chunks, "PRNT") {
        for link in prnt::parse(&chunk.data)? {
            let Some(child) = u32::try_from(link.child).ok().map(Ref::new) else {
                continue;
            };
            if dom.get(child).is_none() {
                continue;
            }

            // A dangling parent is treated like no parent at all: keeping the
            // child at the root is better than dropping it from the tree.
            let parent = if link.parent == prnt::NO_PARENT {
                None
            } else {
                u32::try_from(link.parent)
                    .ok()
                    .map(Ref::new)
                    .filter(|parent| dom.get(*parent).is_some())
            };

            dom.set_parent(child, parent);
        }
    }

    Ok(())
}
