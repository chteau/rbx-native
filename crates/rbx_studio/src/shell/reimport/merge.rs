//! Folding a fresh import into the instance it was first imported as, the
//! way Studio's Reimport does (`creator-docs`, `art/modeling/reimport.md`):
//! a `MeshPart` whose path of names under the model matches one in the file
//! gets the file's mesh and transform and keeps everything else (its
//! `CollisionFidelity`, `RenderFidelity`, colour, scripts under it), a mesh
//! the model lacks is added, and a `MeshPart` the file no longer has is left
//! alone. An incoming mesh with no texture keeps the existing one, and a
//! `SurfaceAppearance` takes the file's maps but keeps its other settings.

use std::collections::HashMap;

use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::clipboard;
use super::super::import3d::place_model;

/// What the file decides on a matching `MeshPart`: the mesh itself, what is
/// derived from it, and where it sits.
const MESH_PROPERTIES: [&str; 10] = [
    "MeshId",
    "MeshContent",
    "InitialSize",
    "PhysicsData",
    "PhysicalConfigData",
    "HasSkinnedMesh",
    "HasJointOffset",
    "JointOffset",
    "Size",
    "CFrame",
];
const TEXTURE_PROPERTIES: [&str; 2] = ["TextureID", "TextureContent"];

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Merged {
    pub(crate) updated: usize,
    pub(crate) added: usize,
}

/// Merges Roblox's model of the file, `imported`, into `target`: placed in a
/// scratch tree first and pivoted onto `target`'s pivot, so a model moved or
/// turned since its import is updated where it now stands.
pub(crate) fn reimport(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    target: Ref,
    imported: &WeakDom,
) -> Result<Merged, String> {
    let mut scratch = WeakDom::new();
    let holder = scratch.new_instance("Folder", "Reimport", None);
    let name = dom
        .get(target)
        .map(|i| i.name().to_string())
        .unwrap_or_default();
    let fresh = place_model(&mut scratch, imported, &name, holder)
        .ok_or("Roblox\u{2019}s import holds nothing to reimport")?;
    if let Some(pivot) = rbx_lua::pivot::pivot(dom, database, target) {
        if let Some(moved) = rbx_lua::pivot::pivot_to(&mut scratch, database, fresh, &pivot) {
            moved?;
        }
    }
    Ok(merge(dom, target, &scratch, fresh))
}

/// [`reimport`]'s second half: `fresh` is already where `target` stands.
pub(crate) fn merge(dom: &mut WeakDom, target: Ref, scratch: &WeakDom, fresh: Ref) -> Merged {
    let existing = by_path(dom, target);
    let mut merged = Merged::default();
    let mut incoming: Vec<_> = by_path(scratch, fresh).into_iter().collect();
    incoming.sort();
    let mut grafted: Vec<Vec<String>> = Vec::new();
    for (path, incoming) in incoming {
        let Some(source) = scratch.get(incoming).filter(|i| i.class() == "MeshPart") else {
            continue;
        };
        // Already carried in with a mesh added above it.
        if grafted.iter().any(|added| path.starts_with(added)) {
            continue;
        }
        match existing.get(&path) {
            Some(&current) if is_mesh_part(dom, current) => {
                for key in MESH_PROPERTIES {
                    if let Some(value) = source.properties().get(key) {
                        let _ = dom.set_property(current, key, value.clone());
                    }
                }
                for key in TEXTURE_PROPERTIES {
                    if let Some(value) = source.properties().get(key).filter(|v| !is_empty(v)) {
                        let _ = dom.set_property(current, key, value.clone());
                    }
                }
                update_surface(dom, current, scratch, incoming);
                merged.updated += 1;
            }
            Some(_) => {}
            None => {
                let parent = existing
                    .get(&path[..path.len() - 1])
                    .copied()
                    .unwrap_or(target);
                if clipboard::graft(dom, scratch, incoming, parent).is_some() {
                    merged.added += 1;
                    grafted.push(path);
                }
            }
        }
    }
    merged
}

fn is_mesh_part(dom: &WeakDom, referent: Ref) -> bool {
    dom.get(referent).is_some_and(|i| i.class() == "MeshPart")
}

fn is_empty(value: &Variant) -> bool {
    match value {
        Variant::String(text) => text.is_empty(),
        Variant::Content(Content::None) => true,
        Variant::Content(Content::Uri(uri)) => uri.is_empty(),
        _ => false,
    }
}

/// The incoming `SurfaceAppearance`'s maps onto the existing one, or the
/// incoming one itself where the part had none.
fn update_surface(dom: &mut WeakDom, part: Ref, scratch: &WeakDom, incoming: Ref) {
    let Some(source) = child_of_class(scratch, incoming, "SurfaceAppearance") else {
        return;
    };
    let Some(current) = child_of_class(dom, part, "SurfaceAppearance") else {
        clipboard::graft(dom, scratch, source, part);
        return;
    };
    let Some(maps) = scratch.get(source).map(|i| {
        i.properties()
            .iter()
            .filter(|(key, value)| is_map(key) && !is_empty(value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    for (key, value) in maps {
        let _ = dom.set_property(current, &key, value);
    }
}

/// `ColorMap`, `NormalMapContent`, `EmissiveMaskContent`…: the images, not
/// the settings beside them.
fn is_map(key: &str) -> bool {
    key.ends_with("Map") || key.ends_with("Content")
}

fn child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&child| dom.get(child).is_some_and(|i| i.class() == class))
}

/// Every descendant of `root` by its path of names below it. Where two
/// siblings share a name the first keeps it, as Studio asks for unique names
/// to tell meshes apart.
fn by_path(dom: &WeakDom, root: Ref) -> HashMap<Vec<String>, Ref> {
    let mut paths = HashMap::new();
    let mut stack = vec![(root, Vec::new())];
    while let Some((referent, path)) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        for &child in instance.children().iter().rev() {
            let Some(name) = dom.get(child).map(|i| i.name().to_string()) else {
                continue;
            };
            let mut below = path.clone();
            below.push(name);
            stack.push((child, below));
        }
        if !path.is_empty() {
            paths.entry(path).or_insert(referent);
        }
    }
    paths
}
