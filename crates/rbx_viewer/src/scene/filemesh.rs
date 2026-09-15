//! Pure DOM extraction for `MeshPart` and `Part`-with-`SpecialMesh`(FileMesh)
//! instances: which mesh/texture assets they need, and how to place the
//! downloaded geometry once it exists. No network or GPU access happens here —
//! see `crate::assets` for downloading and `crate::renderer::filemesh` for the
//! GPU side.

mod appearance;

use std::collections::{HashMap, HashSet};

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::textures::asset_uri;

pub(crate) use appearance::{AlphaMode, Appearance};

use super::material::{Catalog, Slot};

const MESH_PART: &str = "MeshPart";
const FILE_MESH: u32 = 5; // Enum.MeshType.FileMesh

/// How to place a mesh asset's native geometry in the world, before the mesh
/// itself is known.
enum Fit {
    /// `MeshPart`: native geometry is scaled componentwise so its own extent
    /// matches `size` — `InitialSize` if the file carries it (undocumented but
    /// still round-tripped by Studio), otherwise the mesh's own bounds.
    Part {
        cframe: Mat4,
        size: Vec3,
        initial_size: Option<Vec3>,
    },
    /// `SpecialMesh`: native geometry (studs as authored) is scaled by `Scale`
    /// then translated by `Offset`, both in the parent part's own frame. The
    /// part's `size` plays no part in this: unlike `MeshPart`, nothing here
    /// asks the mesh to fit any particular extent.
    Special {
        cframe: Mat4,
        scale: Vec3,
        offset: Vec3,
    },
}

impl Fit {
    fn transform(&self, mesh: &rbx_mesh::Mesh) -> Mat4 {
        match self {
            Fit::Part {
                cframe,
                size,
                initial_size,
            } => {
                let native = initial_size
                    .filter(|v| v.min_element() > f32::EPSILON)
                    .unwrap_or_else(|| Vec3::from(mesh.bounds.size()))
                    .max(Vec3::splat(f32::EPSILON));
                *cframe * Mat4::from_scale(*size / native)
            }
            Fit::Special {
                cframe,
                scale,
                offset,
            } => *cframe * Mat4::from_translation(*offset) * Mat4::from_scale(*scale),
        }
    }
}

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
    pub(crate) meshes: HashMap<AssetRef, rbx_mesh::Mesh>,
    pub(crate) images: HashMap<AssetRef, Image>,
    /// Every distinct `SurfaceAppearance` the scene resolved, deduplicated:
    /// a character's dozen limbs usually share one map set.
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) instances: Vec<ResolvedInstance>,
}

impl Plan {
    /// Every distinct mesh asset this plan needs, in first-seen order.
    pub(crate) fn mesh_refs(&self) -> Vec<AssetRef> {
        dedup(self.entries.iter().map(|entry| &entry.mesh))
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
    meshes: HashMap<AssetRef, rbx_mesh::Mesh>,
    images: HashMap<AssetRef, Image>,
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
    if !database.is_subclass_of(instance.class(), MESH_PART) {
        return None;
    }
    let properties = instance.properties();

    let mesh = parsed_asset_ref(properties.get("MeshId")?)?;
    let appearance = appearance::of(dom, instance);
    // A SurfaceAppearance replaces the mesh's own texture, so reading both
    // would only queue a download nothing goes on to sample.
    let texture = properties
        .get("TextureID")
        .filter(|_| appearance.is_none())
        .and_then(parsed_asset_ref);
    let Some(&Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let initial_size = match properties.get("InitialSize") {
        Some(&Variant::Vector3(v)) => Some(Vec3::new(v.x, v.y, v.z)),
        _ => None,
    };
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
        fit: Fit::Part {
            cframe: super::cframe_matrix(cframe),
            size: Vec3::new(size.x, size.y, size.z),
            initial_size,
        },
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
    let Some(Variant::CFrame(cframe)) = part.properties().get("CFrame") else {
        return None;
    };
    let part_color = match part.properties().get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => super::FALLBACK_COLOR,
    };

    let child = part.children().iter().find_map(|&child_ref| {
        let child = dom.get(child_ref)?;
        (child.class() == "SpecialMesh").then_some(child)
    })?;
    let properties = child.properties();
    if !matches!(properties.get("MeshType"), Some(&Variant::Enum(FILE_MESH))) {
        return None;
    }

    let mesh = parsed_asset_ref(properties.get("MeshId")?)?;
    let texture = properties.get("TextureId").and_then(parsed_asset_ref);
    let scale = vector3(properties.get("Scale"), Vec3::ONE);
    let offset = vector3(properties.get("Offset"), Vec3::ZERO);
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
        fit: Fit::Special {
            cframe: super::cframe_matrix(cframe),
            scale,
            offset,
        },
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
