//! Pure DOM extraction for `MeshPart` and `Part`-with-`SpecialMesh`(FileMesh)
//! instances: which mesh/texture assets they need, and how to place the
//! downloaded geometry once it exists. No network or GPU access happens here —
//! see `crate::assets` for downloading and `crate::renderer::filemesh` for the
//! GPU side.

mod appearance;
mod fit;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::textures::asset_uri;

pub(crate) use appearance::{AlphaMode, Appearance};
pub(crate) use fit::{of as fit_of, Fit};

use super::material::{Catalog, Slot};

const MESH_PART: &str = "MeshPart";
const FILE_MESH: u32 = 5; // Enum.MeshType.FileMesh

/// One file-mesh-backed instance found in the DOM, before its assets exist.
pub(super) struct Entry {
    material: Slot,
    /// The drawn `Part` this entry stands in for: the `MeshPart` itself, or
    /// the parent of a `SpecialMesh` child. Used to find and hide that part's
    /// fallback box once real geometry resolves — see `Scene::resolve_file_meshes`.
    referent: Ref,
    mesh: AssetRef,
    /// The mesh's own `TextureID`, and `None` whenever `appearance` is set: a
    /// `SurfaceAppearance` replaces it outright rather than layering over it.
    texture: Option<AssetRef>,
    appearance: Option<Appearance>,
    fit: Fit,
    color: [f32; 3],
    alpha: f32,
    reflectance: f32,
    casts_shadow: bool,
}

/// Every file-mesh instance in a DOM, extracted once and reused both to list
/// the assets a caller must download and to resolve them afterward.
#[derive(Default)]
pub(crate) struct Plan {
    entries: Vec<Entry>,
}

/// One instance ready for the renderer: which downloaded mesh/texture it
/// draws, its model matrix, and its tint.
///
/// `Clone`: a union's instances outlive the resolved set they sit in, which a
/// streaming load rebuilds from the file mesh plan every time more meshes
/// land — see `scene::union::Merged`.
#[derive(Clone)]
pub(crate) struct ResolvedInstance {
    /// The part this instance draws in place of (see [`Entry::referent`]), so
    /// the renderer's per-instance patch maps and `Scene::patch_mesh_instance`
    /// can find it by the same id the Properties panel edits.
    pub(crate) referent: Ref,
    pub(crate) mesh: AssetRef,
    /// Re-read by [`super::Scene::resolve_materials`] once the packs are in.
    pub(crate) material: Slot,
    pub(crate) texture: Option<AssetRef>,
    /// Index into [`Resolved::appearances`], which the renderer turns into one
    /// bind group per distinct map set.
    pub(crate) appearance: Option<usize>,
    pub(crate) model: Mat4,
    pub(crate) color: [f32; 3],
    /// `1 - Transparency`, always above zero: an invisible instance is dropped
    /// by [`resolve`] rather than handed to the renderer to blend away.
    pub(crate) alpha: f32,
    pub(crate) reflectance: f32,
    /// `BasePart.CastShadow` of the part this mesh stands for — see
    /// [`super::Part::casts_shadow`].
    pub(crate) casts_shadow: bool,
}

/// A [`Plan`] joined to whatever meshes and images actually downloaded.
///
/// Carries the images too, not just their references: `Renderer::new` only
/// ever gets a `&Scene`/`&Decor` pair, so this is the one place mesh textures
/// can ride along to reach `renderer::filemesh`.
#[derive(Default)]
pub(crate) struct Resolved {
    /// Behind `Arc`s so a hit test on another thread (see
    /// `crate::pick::Meshes`) reads the very vertices the renderer uploads
    /// rather than a copy of every mesh in the place — and so the decoded
    /// mesh kept across reloads (see `load::Resident`) is that same one.
    pub(crate) meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
    /// Behind `Arc`s for the second of those reasons.
    pub(crate) images: HashMap<AssetRef, Arc<Image>>,
    /// Every distinct `SurfaceAppearance` the scene resolved, deduplicated:
    /// a character's dozen limbs usually share one map set.
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) instances: Vec<ResolvedInstance>,
}

impl Entry {
    /// The mesh this entry draws and every image it samples — its own
    /// `TextureID` and, where it has one, the four `SurfaceAppearance` maps.
    pub(super) fn assets(&self) -> (AssetRef, Vec<AssetRef>) {
        let images = self
            .texture
            .iter()
            .chain(
                self.appearance
                    .iter()
                    .flat_map(|appearance| appearance.maps.iter().flatten()),
            )
            .cloned()
            .collect();
        (self.mesh.clone(), images)
    }
}

impl Plan {
    /// Every distinct mesh asset this plan needs, in first-seen order.
    pub(crate) fn mesh_refs(&self) -> Vec<AssetRef> {
        dedup(self.entries.iter().map(|entry| &entry.mesh))
    }

    /// Swaps one part's entry for `entry`, in the place the old one held, or
    /// drops it where the part is no longer file-mesh-backed at all.
    ///
    /// A plan is made once from the DOM and joined to assets again every time
    /// more of them land (see `load::Loaded::resolve`), long after the DOM is
    /// out of reach — so a single-instance edit that changes what an instance
    /// draws has to leave the plan saying so, or the next landing would
    /// resolve the `MeshId` the file was opened with.
    pub(super) fn install(&mut self, referent: Ref, entry: Option<Entry>) {
        let at = self
            .entries
            .iter()
            .position(|held| held.referent == referent);
        match (at, entry) {
            // In place: the plan's order is the order `resolve` builds
            // instances in, which is the order the renderer batches them in.
            (Some(at), Some(entry)) => self.entries[at] = entry,
            (Some(at), None) => {
                self.entries.remove(at);
            }
            (None, Some(entry)) => self.entries.push(entry),
            (None, None) => {}
        }
    }

    /// Every distinct image asset this plan needs, in first-seen order: the
    /// meshes' own textures and the `SurfaceAppearance` maps alike, since both
    /// download through the same pool.
    pub(crate) fn texture_refs(&self) -> Vec<AssetRef> {
        dedup(self.entries.iter().flat_map(|entry| {
            entry.texture.iter().chain(
                entry
                    .appearance
                    .iter()
                    .flat_map(|appearance| appearance.maps.iter().flatten()),
            )
        }))
    }
}

fn dedup<'a>(refs: impl Iterator<Item = &'a AssetRef>) -> Vec<AssetRef> {
    let mut seen = Vec::new();
    for reference in refs {
        if !seen.contains(reference) {
            seen.push(reference.clone());
        }
    }
    seen
}

/// Walks a DOM for every `MeshPart` and every `SpecialMesh` FileMesh child of
/// a plain part.
///
/// Workspace-scoped, same as `Scene::from_dom`'s own part build: a mesh part
/// staged outside `Workspace` never draws, so there is nothing to plan for it.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase, materials: &mut Catalog) -> Plan {
    let entries = super::workspace_descendants(dom, database)
        .filter(|&referent| super::is_drawable(dom, database, referent))
        .filter_map(|referent| {
            from_mesh_part(dom, database, referent, materials)
                .or_else(|| from_special_mesh_child(dom, database, referent, materials))
        })
        .collect();
    Plan { entries }
}

/// Resolves a plan against downloaded assets.
///
/// Returns the renderer-ready instances alongside the set of referents that
/// got real geometry, so the fallback box of everything else can stay put.
pub(crate) fn resolve(
    plan: &Plan,
    meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
    images: HashMap<AssetRef, Arc<Image>>,
) -> (Resolved, HashSet<Ref>) {
    let mut instances = Vec::new();
    let mut appearances: Vec<Appearance> = Vec::new();
    let mut hidden = HashSet::new();

    for entry in &plan.entries {
        let Some(mesh) = meshes.get(&entry.mesh) else {
            continue;
        };
        // Hidden either way: a fully transparent MeshPart draws nothing at
        // all, and its fallback box reappearing would be worse than nothing.
        hidden.insert(entry.referent);
        if entry.alpha <= 0.0 {
            continue;
        }

        let texture = entry
            .texture
            .clone()
            .filter(|reference| images.contains_key(reference));
        let appearance = entry.appearance.as_ref().map(|planned| {
            let resolved = planned.resolved(&images);
            match appearances.iter().position(|known| *known == resolved) {
                Some(slot) => slot,
                None => {
                    appearances.push(resolved);
                    appearances.len() - 1
                }
            }
        });
        instances.push(ResolvedInstance {
            referent: entry.referent,
            mesh: entry.mesh.clone(),
            material: entry.material,
            texture,
            appearance,
            model: entry.fit.transform(mesh),
            color: entry.color,
            alpha: entry.alpha,
            reflectance: entry.reflectance,
            casts_shadow: entry.casts_shadow,
        });
    }

    (
        Resolved {
            meshes,
            images,
            appearances,
            instances,
        },
        hidden,
    )
}

fn from_mesh_part(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    let instance = dom.get(referent)?;
    let (mesh, fit) = fit::mesh_part(database, instance)?;
    let properties = instance.properties();

    let appearance = appearance::of(dom, instance);
    // A SurfaceAppearance replaces the mesh's own texture, so reading both
    // would only queue a download nothing goes on to sample.
    let texture = properties
        .get("TextureID")
        .filter(|_| appearance.is_none())
        .and_then(parsed_asset_ref);
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => super::FALLBACK_COLOR,
    };

    Some(Entry {
        referent,
        material: materials.slot_for(properties, database),
        mesh,
        texture,
        appearance,
        fit,
        color: color.map(|channel| super::srgb_to_linear(f32::from(channel) / 255.0)),
        alpha: 1.0 - super::number(properties.get("Transparency")).clamp(0.0, 1.0),
        reflectance: super::number(properties.get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(properties),
    })
}

/// A plain `Part` wearing a `SpecialMesh` FileMesh child: the child replaces
/// the part's geometry outright, same precedence as `scene::shape::resolve`.
/// Only the FileMesh case reaches here — every other `MeshType` already has a
/// procedural stand-in built in `shape::resolve` and is left alone.
fn from_special_mesh_child(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    let part = dom.get(referent)?;
    let (child, mesh, fit) = fit::special_mesh(dom, part)?;
    let part_color = match part.properties().get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => super::FALLBACK_COLOR,
    };

    let properties = child.properties();
    let texture = properties.get("TextureId").and_then(parsed_asset_ref);
    let tint = vector3(properties.get("VertexColor"), Vec3::ONE);

    let color = std::array::from_fn(|axis| {
        let part_channel = super::srgb_to_linear(f32::from(part_color[axis]) / 255.0);
        part_channel * super::srgb_to_linear(tint.to_array()[axis])
    });

    Some(Entry {
        referent,
        // The part's material, not the mesh's: a SpecialMesh has none.
        material: materials.slot_for(part.properties(), database),
        mesh,
        texture,
        // `SurfaceAppearance` is a MeshPart child; a SpecialMesh never has one.
        appearance: None,
        fit,
        color,
        // A SpecialMesh has no Transparency of its own: the part it hangs under
        // owns both properties.
        alpha: 1.0 - super::number(part.properties().get("Transparency")).clamp(0.0, 1.0),
        reflectance: super::number(part.properties().get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(part.properties()),
    })
}

fn vector3(value: Option<&Variant>, default: Vec3) -> Vec3 {
    match value {
        Some(&Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => default,
    }
}

/// Reads a `MeshId`/`TextureID`/`TextureId` property, empty references included
/// as `None` since Roblox uses the empty string for "no asset".
fn parsed_asset_ref(value: &Variant) -> Option<AssetRef> {
    match AssetRef::parse(asset_uri(value)?).ok()? {
        AssetRef::Empty => None,
        reference => Some(reference),
    }
}

mod patch;
pub(super) use patch::replan;

#[cfg(test)]
#[path = "filemesh/tests.rs"]
mod tests;
