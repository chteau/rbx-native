//! Layered clothing, wrapped to the body.
//!
//! A layered garment's `WrapLayer` carries its inner cage (`ReferenceMeshId`,
//! in the garment's own frame, shifted by `ReferenceOrigin`) and its outer
//! cage (`CageMeshId`, shifted by `CageOrigin`); each body part of the
//! character it is worn on carries a `WrapTarget` whose `CageMeshId` is the
//! body as it is now. Roblox fits the garment cage to cage: the inner cage
//! goes onto the body cage vertex for vertex, and a layer above goes onto the
//! outer cage of the layer below. [`cage_map`] does that pairing by template
//! UV and binds each garment vertex to the closest spot on its inner cage,
//! rebuilt at the same spot of the body with the same clearance; layers are fitted lowest `Order` first, each one's outer cage laid over
//! the surface the next one is worn on. Then each vertex rises a little
//! along its normal by the layer's `Order`, so stacked layers do not fight
//! over one surface.
//!
//! An inner cage that is not made from the body's template (no UV pairing)
//! falls back to the nearest-vertex fit: every reference vertex is matched to
//! the nearest body cage vertex in the world, and each garment vertex moves
//! by the inverse-distance blend of the four nearest reference vertices'
//! displacements. The reference cage rests in the garment's frame, which need
//! not be where the body stands (the real Black Detective Trench Coat's sits
//! 0.8 studs high), so for that fit the reference is first slid onto the body
//! (`nearest::register`); matching vertices across that gap would pick the wrong
//! surface and tear the garment.
//!
//! `BindOffset` moves the fitted garment as a whole, a `CFrame` about the
//! character's `HumanoidRootPart`: Studio, given `(0, 0.5, 0)`, wears the
//! catalog Denim Jacket (8516609675) half a stud higher, unchanged in shape,
//! and given `CFrame.Angles(0, 0, 0.3)` turns it 0.3 radians about the root
//! part, not the handle. The cages stay where they are, so the layer above
//! is worn over the same outer cage.
//!
//! `Puffiness` and `ShrinkFactor` are not applied, as Studio applies neither:
//! <https://create.roblox.com/docs/reference/engine/classes/WrapLayer> marks
//! both deprecated, and in Studio the same jacket draws pixel for pixel the
//! same at `Puffiness` -1, 0.5 and 1 and `ShrinkFactor` -1, 0.5 and 1, alone
//! on the body or over a second layer. Studio clamps both to -1..1.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use self::cage_map::{CageMap, Key, Surface, WorldCage};
pub(crate) use self::hsr::{cull, Hsr};
use self::nearest::deform;
use super::fit::{self, Fit};
use super::{parsed_asset_ref, Entry};
use crate::scene::cframe_matrix;

/// Neighbours blended per garment vertex.
const BLEND: usize = 4;
/// Studs of lift per `Order` step.
const LIFT: f32 = 0.004;

#[derive(Clone)]
pub(super) struct Wrap {
    reference: AssetRef,
    /// `ReferenceOrigin`, in the garment's mesh space.
    origin: Mat4,
    /// `CageMeshId` and `CageOrigin`: the outer cage the layer above is
    /// worn over.
    outer: Option<(AssetRef, Mat4)>,
    order: f32,
    /// `BindOffset` as a world transform about the character's root part,
    /// `None` when it is the identity.
    bind: Option<Mat4>,
    targets: Vec<Target>,
    /// The layer's hidden surface data, see [`hsr`].
    hsr: Option<HsrSource>,
    /// The character the garment is worn on: the handle's accessory's parent,
    /// whose parts the wrapped garment is skinned to.
    pub(super) character: Ref,
}

#[derive(Clone)]
struct Target {
    cage: AssetRef,
    origin: Mat4,
    /// The body part's own mesh and fit: the cage shares its space.
    mesh: AssetRef,
    fit: Fit,
    /// The body part's `Name`: the bone its cage's vertices lend the garment.
    part: String,
    /// The body part itself, whose faces under the clothing are hidden.
    referent: Ref,
    hsr: Option<HsrSource>,
}

/// Where a wrap's hidden surface data is: inline in `HSRData`, or in the
/// `HiddenSurfaceRemovalAsset` its `HSRAssetId` names.
#[derive(Clone)]
enum HsrSource {
    Inline(Arc<Hsr>),
    Asset(AssetRef),
}

impl HsrSource {
    fn of(wrap: &rbx_dom::Instance) -> Option<HsrSource> {
        let properties = wrap.properties();
        match hsr::data(properties.get("HSRData")).and_then(Hsr::parse) {
            Some(inline) => Some(HsrSource::Inline(Arc::new(inline))),
            None => parsed_asset_ref(properties.get("HSRAssetId")?).map(HsrSource::Asset),
        }
    }

    fn get<'a>(&'a self, assets: &'a HashMap<AssetRef, Arc<Hsr>>) -> Option<&'a Hsr> {
        match self {
            HsrSource::Inline(inline) => Some(inline),
            HsrSource::Asset(asset) => assets.get(asset).map(|hsr| &**hsr),
        }
    }

    fn asset(&self) -> Option<&AssetRef> {
        match self {
            HsrSource::Asset(asset) => Some(asset),
            HsrSource::Inline(_) => None,
        }
    }
}

/// A cage's asset under `key`. Places Studio saves now hold it as the
/// `...Content` property that replaced `...Id` (`CageMeshContent`,
/// `ReferenceMeshContent`), so that is read when `key` is missing.
fn cage_ref(node: &Instance, key: &str) -> Option<AssetRef> {
    let props = node.properties();
    let renamed = key.strip_suffix("Id").map(|stem| format!("{stem}Content"));
    parsed_asset_ref(props.get(key).or_else(|| props.get(renamed.as_deref()?))?)
}

/// The wrap of `handle`, when it carries a `WrapLayer` with a reference cage
/// and sits in a character that has cages to wrap to.
pub(super) fn of(dom: &WeakDom, database: &ReflectionDatabase, handle: Ref) -> Option<Wrap> {
    let layer = dom
        .get(handle)?
        .children()
        .iter()
        .filter_map(|&c| dom.get(c))
        .find(|c| c.class() == "WrapLayer")?;
    let reference = cage_ref(layer, "ReferenceMeshId")?;
    let hsr = HsrSource::of(layer);
    let origin = match layer.properties().get("ReferenceOrigin") {
        Some(Variant::CFrame(c)) => cframe_matrix(c),
        _ => Mat4::IDENTITY,
    };
    let outer = cage_ref(layer, "CageMeshId").map(|cage| {
        let origin = match layer.properties().get("CageOrigin") {
            Some(Variant::CFrame(c)) => cframe_matrix(c),
            _ => Mat4::IDENTITY,
        };
        (cage, origin)
    });
    let order = crate::scene::number(layer.properties().get("Order"));
    let rig = dom.parent(dom.parent(handle)?)?;
    let bind = match layer.properties().get("BindOffset") {
        Some(Variant::CFrame(c)) if cframe_matrix(c) != Mat4::IDENTITY => {
            let root = dom
                .get(rig)?
                .children()
                .iter()
                .filter_map(|&c| dom.get(c))
                .find(|c| c.name() == "HumanoidRootPart")
                .or_else(|| dom.get(handle))
                .and_then(|p| match p.properties().get("CFrame") {
                    Some(Variant::CFrame(c)) => Some(cframe_matrix(c)),
                    _ => None,
                })
                .unwrap_or(Mat4::IDENTITY);
            Some(root * cframe_matrix(c) * root.inverse())
        }
        _ => None,
    };
    let targets: Vec<Target> = dom
        .get(rig)?
        .children()
        .iter()
        .filter_map(|&referent| {
            let part = dom.get(referent)?;
            let (mesh, fit) = fit::mesh_part(database, part)?;
            let name = part.name().to_owned();
            let target = part
                .children()
                .iter()
                .filter_map(|&c| dom.get(c))
                .find(|c| c.class() == "WrapTarget")?;
            Some(Target {
                cage: cage_ref(target, "CageMeshId")?,
                origin: match target.properties().get("CageOrigin") {
                    Some(Variant::CFrame(c)) => cframe_matrix(c),
                    _ => Mat4::IDENTITY,
                },
                mesh,
                fit,
                part: name,
                referent,
                hsr: HsrSource::of(target),
            })
        })
        .collect();
    (!targets.is_empty()).then_some(Wrap {
        reference,
        origin,
        outer,
        order,
        bind,
        targets,
        hsr,
        character: rig,
    })
}

impl Wrap {
    /// The meshes the wrap needs besides the garment's own.
    pub(super) fn meshes(&self) -> impl Iterator<Item = &AssetRef> {
        std::iter::once(&self.reference)
            .chain(self.targets.iter().flat_map(|t| [&t.cage, &t.mesh]))
            .chain(self.outer.iter().map(|(cage, _)| cage))
    }

    /// The `HiddenSurfaceRemovalAsset`s the wrap and its body name.
    pub(super) fn hsr_assets(&self) -> impl Iterator<Item = &AssetRef> {
        self.hsr
            .iter()
            .chain(self.targets.iter().flat_map(|t| t.hsr.iter()))
            .filter_map(HsrSource::asset)
    }
}

pub(super) fn key(referent: Ref) -> AssetRef {
    AssetRef::Thumb(format!("wrapped-mesh/{referent:?}"))
}

/// The key a body part's mesh is drawn under once faces are hidden.
pub(super) fn hidden_key(referent: Ref) -> AssetRef {
    AssetRef::Thumb(format!("hidden-surfaces/{referent:?}"))
}

/// What the wraps of a place make: every wrapped garment's deformed mesh,
/// and the faces hidden under the clothing (see [`hsr`]) of each body part
/// that has any.
#[derive(Default)]
pub(super) struct Wrapped {
    pub(super) garments: HashMap<Ref, (AssetRef, Arc<rbx_mesh::SkinnedMesh>)>,
    pub(super) hidden: HashMap<Ref, Vec<bool>>,
}

/// Every wrapped garment's deformed mesh, by referent, for those whose cages
/// have all arrived. Each character's garments are fitted lowest `Order`
/// first, over the surface the ones below left. Each is skinned to the
/// character's parts: through the garment's own skin when its bones name the
/// body parts it wraps, otherwise through one lent by the body (see
/// [`lent_skin`]). Faces under a higher layer are left out of each garment,
/// and of the body under all of them, by the HSR data in `surfaces` or
/// inline.
pub(super) fn derive(
    entries: &[Entry],
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    surfaces: &HashMap<AssetRef, Arc<Hsr>>,
) -> Wrapped {
    let mut worn: Vec<&Entry> = entries.iter().filter(|e| e.wrap.is_some()).collect();
    worn.sort_by(|a, b| {
        let order = |e: &Entry| e.wrap.as_ref().map_or(0.0, |w| w.order);
        order(a).total_cmp(&order(b))
    });
    let mut surface_of: HashMap<Ref, Surface> = HashMap::new();
    let mut out = Wrapped::default();
    for entry in &worn {
        let Some(wrap) = entry.wrap.as_ref() else {
            continue;
        };
        let Some(garment) = meshes.get(&entry.mesh) else {
            continue;
        };
        let model = entry.fit.transform(garment);
        let surface = match surface_of.entry(wrap.character) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(body_surface(wrap, meshes))
            }
        };
        let Some(mut deformed) = deform_over(wrap, model, garment, meshes, surface) else {
            continue;
        };
        if let Some(bind) = wrap.bind {
            deformed = rebound(&deformed, model.inverse() * bind * model);
        }
        let skin = match &garment.skin {
            Some(own) if names_the_body(wrap, own) => Some(own.clone()),
            _ => lent_skin(wrap, model, &deformed, meshes),
        };
        let mut fitted = rbx_mesh::SkinnedMesh {
            mesh: deformed,
            skin,
        };
        let above = covered(&worn, wrap.character, Some(wrap.order), meshes, surfaces);
        let outer = wrap.outer.as_ref().and_then(|(cage, _)| meshes.get(cage));
        if let (Some(hsr), Some(outer)) = (wrap.hsr.as_ref().and_then(|h| h.get(surfaces)), outer) {
            fitted = hsr::cull(&fitted, &hsr.hidden(outer, &above));
        }
        out.garments
            .insert(entry.referent, (key(entry.referent), Arc::new(fitted)));
    }
    // The body, under every layer of its character.
    let mut seen = HashSet::new();
    for wrap in worn.iter().filter_map(|e| e.wrap.as_ref()) {
        if !seen.insert(wrap.character) {
            continue;
        }
        let all = covered(&worn, wrap.character, None, meshes, surfaces);
        for target in &wrap.targets {
            let (Some(hsr), Some(cage)) = (
                target.hsr.as_ref().and_then(|h| h.get(surfaces)),
                meshes.get(&target.cage),
            ) else {
                continue;
            };
            let hidden = hsr.hidden(cage, &all);
            if hidden.contains(&true) {
                out.hidden.insert(target.referent, hidden);
            }
        }
    }
    out
}

/// The template UVs the layers worn on `character` cover: those above
/// `order`, or all of them for the body.
fn covered(
    worn: &[&Entry],
    character: Ref,
    order: Option<f32>,
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    surfaces: &HashMap<AssetRef, Arc<Hsr>>,
) -> HashSet<Key> {
    worn.iter()
        .filter_map(|e| e.wrap.as_ref())
        .filter(|w| w.character == character && order.is_none_or(|o| w.order > o))
        .filter_map(|w| Some((w.hsr.as_ref()?.get(surfaces)?, meshes.get(&w.reference)?)))
        .flat_map(|(hsr, inner)| hsr.covered(inner).collect::<Vec<_>>())
        .collect()
}

/// `garment` moved by `by`, in its own mesh space: the fitted garment as
/// `BindOffset` moves it.
fn rebound(garment: &rbx_mesh::Mesh, by: Mat4) -> rbx_mesh::Mesh {
    let mut out = garment.clone();
    for vertex in &mut out.vertices {
        vertex.position = by.transform_point3(Vec3::from(vertex.position)).to_array();
        vertex.normal = by
            .transform_vector3(Vec3::from(vertex.normal))
            .normalize_or_zero()
            .to_array();
    }
    out
}

/// The body's cages as the surface the first layer is worn over.
fn body_surface(wrap: &Wrap, meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>) -> Surface {
    let mut surface = Surface::default();
    for target in &wrap.targets {
        // A part whose mesh or cage didn't load (a head behind
        // authentication) leaves a hole, not a bare body.
        let (Some(cage), Some(part)) = (meshes.get(&target.cage), meshes.get(&target.mesh)) else {
            continue;
        };
        let to_world = target.fit.transform(part) * target.origin;
        surface.lay(&world_cage(cage, to_world));
    }
    surface
}

fn world_cage(cage: &rbx_mesh::Mesh, to_world: Mat4) -> WorldCage<'_> {
    WorldCage {
        positions: cage
            .vertices
            .iter()
            .map(|v| to_world.transform_point3(Vec3::from(v.position)))
            .collect(),
        mesh: cage,
    }
}

/// The garment fitted over `surface` by its cages, then its outer cage laid
/// over `surface` for the layer above. Falls back to [`deform`] when the
/// inner cage does not pair with the surface.
fn deform_over(
    wrap: &Wrap,
    model: Mat4,
    garment: &rbx_mesh::Mesh,
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    surface: &mut Surface,
) -> Option<rbx_mesh::Mesh> {
    let reference = meshes.get(&wrap.reference)?;
    let inner = world_cage(reference, model * wrap.origin);
    let Some(map) = CageMap::pair(&inner, surface) else {
        return deform(wrap, model, garment, meshes);
    };
    let back = model.inverse();
    let lift = LIFT * (wrap.order + 1.0);
    let mut out = garment.clone();
    for vertex in &mut out.vertices {
        let at = model.transform_point3(Vec3::from(vertex.position));
        let normal = model
            .transform_vector3(Vec3::from(vertex.normal))
            .normalize_or_zero();
        let (moved, turned) = map.carry(at, normal);
        vertex.position = back.transform_point3(moved + turned * lift).to_array();
        vertex.normal = back
            .transform_vector3(turned)
            .normalize_or_zero()
            .to_array();
    }
    if let Some((cage, origin)) = &wrap.outer {
        if let Some(outer) = meshes.get(cage) {
            let mut laid = world_cage(outer, model * *origin);
            for p in &mut laid.positions {
                *p = map.carry(*p, Vec3::ZERO).0;
            }
            surface.lay(&laid);
        }
    }
    Some(out)
}

/// Every body cage vertex where the wrap lays it, with the target it is
/// from; `None` until every cage and body mesh has arrived.
fn body(
    wrap: &Wrap,
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
) -> Option<(Vec<Vec3>, Vec<u16>)> {
    let (mut points, mut owners) = (Vec::new(), Vec::new());
    for (index, target) in wrap.targets.iter().enumerate() {
        // A part whose mesh or cage didn't load (a head behind
        // authentication) leaves a hole, not a bare body.
        let (Some(cage), Some(part)) = (meshes.get(&target.cage), meshes.get(&target.mesh)) else {
            continue;
        };
        let to_world = target.fit.transform(part) * target.origin;
        points.extend(
            cage.vertices
                .iter()
                .map(|v| to_world.transform_point3(Vec3::from(v.position))),
        );
        owners.resize(points.len(), index as u16);
    }
    Some((points, owners))
}

/// Whether a bone of `skin` is named after a body part the garment wraps.
fn names_the_body(wrap: &Wrap, skin: &rbx_mesh::Skin) -> bool {
    skin.bones
        .iter()
        .any(|bone| wrap.targets.iter().any(|target| target.part == bone.name))
}

/// A skin for a garment without one of its own: each wrapped vertex takes
/// the parts of the [`BLEND`] body cage vertices nearest it, by inverse
/// square distance, one bone per wrapped body part (bound at the identity,
/// which the rig's binding cancels out anyway).
fn lent_skin(
    wrap: &Wrap,
    model: Mat4,
    wrapped: &rbx_mesh::Mesh,
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
) -> Option<rbx_mesh::Skin> {
    let (body, owners) = body(wrap, meshes)?;
    if body.is_empty() || wrap.targets.len() > usize::from(u16::MAX) {
        return None;
    }
    let influences = wrapped
        .vertices
        .iter()
        .map(|vertex| {
            let at = model.transform_point3(Vec3::from(vertex.position));
            let mut near = [(f32::INFINITY, 0u16); BLEND];
            for (point, &owner) in body.iter().zip(&owners) {
                let d = point.distance_squared(at);
                if d < near[BLEND - 1].0 {
                    near[BLEND - 1] = (d, owner);
                    near.sort_by(|a, b| a.0.total_cmp(&b.0));
                }
            }
            let mut merged: Vec<(u16, f32)> = Vec::with_capacity(BLEND);
            for (d, owner) in near.iter().filter(|(d, _)| d.is_finite()) {
                let w = 1.0 / (d + 1e-6);
                match merged.iter_mut().find(|(bone, _)| bone == owner) {
                    Some((_, weight)) => *weight += w,
                    None => merged.push((*owner, w)),
                }
            }
            let total: f32 = merged.iter().map(|(_, w)| w).sum();
            let mut influence = rbx_mesh::Influences {
                bones: [0; 4],
                weights: [0.0; 4],
            };
            for (slot, (bone, weight)) in merged.into_iter().enumerate() {
                influence.bones[slot] = bone;
                influence.weights[slot] = weight / total;
            }
            influence
        })
        .collect();
    Some(rbx_mesh::Skin {
        bones: wrap
            .targets
            .iter()
            .map(|target| rbx_mesh::Bone {
                name: target.part.clone(),
                parent: None,
                lod_parent: None,
                culling: 0.0,
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                position: [0.0; 3],
            })
            .collect(),
        subsets: Vec::new(),
        envelopes: Vec::new(),
        influences,
        facs: None,
    })
}

mod cage_map;
mod hsr;
mod nearest;

#[cfg(test)]
#[path = "wrap/tests.rs"]
mod tests;
