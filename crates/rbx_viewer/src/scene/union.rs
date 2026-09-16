//! Legacy `UnionOperation`/`NegateOperation` handling: recovers the original,
//! pre-CSG parts from the `PartOperationAsset` a builder's union was baked
//! into and recomputes the boolean over them (`csg`), instead of drawing the
//! union as a plain box forever. Roblox's own baked `MeshData` is never read.
//!
//! Two-phase like `filemesh`: [`plan`] walks the DOM for every operation that
//! carries a legacy `AssetId`, and [`resolve`] joins that plan against
//! downloaded asset bytes once they exist. No network access happens here —
//! see `crate::assets` for downloading.

mod csg;
mod tree;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::textures::asset_uri;

use super::material::{Catalog, Slot};
use super::{Part, ResolvedInstance};

const PART_OPERATION: &str = "PartOperation";

/// A place with many legacy unions shouldn't spawn one thread per boolean
/// regardless of core count — same bounded-worker-pool shape as
/// `crate::assets`'s download pool, just sized to available CPU parallelism
/// rather than a remote request-rate limit: this work is CPU-bound, not
/// network-bound, so the cap that matters is core count, not Roblox's API.
const MAX_CSG_WORKERS: usize = 8;

/// One legacy union/negate found in the DOM, before its asset exists.
pub(super) struct Entry {
    /// The real DOM instance this entry stands for, so its fallback box can be
    /// hidden once real geometry resolves — see `Scene::resolve_unions`.
    referent: Ref,
    asset: AssetRef,
    /// The union's own world placement: everything the asset's operation tree
    /// describes is expressed relative to this.
    cframe: Mat4,
    /// A builder can resize a union after Studio baked its CSG tree, the same
    /// way a `MeshPart` can be resized after import — see `filemesh::Fit::Part`.
    /// The tree's own coordinates are authored against `initial_size`, so the
    /// ratio to current `size` is the extra scale that resize left behind.
    size: Vec3,
    initial_size: Vec3,
    /// The union's own look, captured up front the way `filemesh::Entry` does:
    /// the computed mesh is one instance for the renderer, not a `Part`.
    material: Slot,
    /// `UsePartColor` on: the union's own colour paints the whole result; off:
    /// the pieces keep theirs (see `csg::largest_additive_color`).
    color: Option<[u8; 3]>,
    alpha: f32,
    reflectance: f32,
    casts_shadow: bool,
}

impl Entry {
    /// Unit mesh (asset frame) to world — see `initial_size`.
    fn placement(&self) -> Mat4 {
        self.cframe * Mat4::from_scale(self.size / self.initial_size)
    }

    /// The renderer's instance for this union's computed mesh, painted
    /// `color` (linear). Shared by [`resolve`] and [`Entry::patched`] so the
    /// two can never disagree on what a union instance carries.
    fn instance(&self, color: [f32; 3]) -> ResolvedInstance {
        ResolvedInstance {
            referent: self.referent,
            mesh: self.asset.clone(),
            material: self.material,
            texture: None,
            appearance: None,
            model: self.placement(),
            color,
            alpha: self.alpha,
            reflectance: self.reflectance,
            casts_shadow: self.casts_shadow,
        }
    }

    /// Whether a fresh [`resolve`] would drop this entry as fully transparent
    /// — see `filemesh::Entry::is_invisible`.
    pub(super) fn is_invisible(&self) -> bool {
        self.alpha <= 0.0
    }

    /// The instance [`resolve`] would build for this entry, assuming its
    /// boolean geometry already computed — `None` where a fresh resolution
    /// would drop it (fully transparent), or where its colour would have to
    /// come from the operation tree (`UsePartColor` off — see [`resolve`]),
    /// which only exists while that tree is being evaluated.
    pub(super) fn patched(&self) -> Option<ResolvedInstance> {
        if self.alpha <= 0.0 {
            return None;
        }
        let color = self.color?;
        Some(self.instance(color.map(|channel| super::srgb_to_linear(f32::from(channel) / 255.0))))
    }
}

/// The single-instance counterpart of [`plan`] — see `filemesh::replan`.
pub(super) fn replan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    from_operation(dom, database, referent, materials)
}

/// Every legacy union/negate in a DOM, extracted once and reused both to list
/// the assets a caller must download and to resolve them afterward.
#[derive(Default)]
pub(crate) struct Plan {
    entries: Vec<Entry>,
}

impl Plan {
    /// One `(referent, asset)` pair per union found, in first-seen order.
    /// Several unions can share the same `AssetId` (a builder copy-pasting a
    /// rock, say); callers should dedupe before downloading, same as
    /// `filemesh`'s `mesh_refs`/`texture_refs`.
    pub(crate) fn assets(&self) -> Vec<(Ref, AssetRef)> {
        self.entries
            .iter()
            .map(|entry| (entry.referent, entry.asset.clone()))
            .collect()
    }
}

/// Walks a DOM for every `PartOperation` (`UnionOperation`, `NegateOperation`)
/// that still carries the legacy `AssetId` its pre-CSG tree was baked into.
///
/// Workspace-scoped, same as `Scene::from_dom`'s own part build: a union
/// staged outside `Workspace` never draws, so there is nothing to plan for it.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase, materials: &mut Catalog) -> Plan {
    let entries = super::workspace_descendants(dom, database)
        .filter(|&referent| super::is_drawable(dom, database, referent))
        .filter_map(|referent| from_operation(dom, database, referent, materials))
        .collect();
    Plan { entries }
}

fn from_operation(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    let instance = dom.get(referent)?;
    if !database.is_subclass_of(instance.class(), PART_OPERATION) {
        return None;
    }
    let properties = instance.properties();

    let asset = match AssetRef::parse(asset_uri(properties.get("AssetId")?)?).ok()? {
        AssetRef::Empty => return None,
        asset => asset,
    };
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let Some(&Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let size = Vec3::new(size.x, size.y, size.z);
    let initial_size = match properties.get("InitialSize") {
        Some(&Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => size,
    }
    .max(Vec3::splat(f32::EPSILON));
    let use_part_color = matches!(properties.get("UsePartColor"), Some(Variant::Bool(true)));
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) if use_part_color => Some([r, g, b]),
        _ => None,
    };

    Some(Entry {
        referent,
        asset,
        cframe: super::cframe_matrix(cframe),
        size,
        initial_size,
        material: materials.slot_for(properties, database),
        color,
        alpha: 1.0 - super::number(properties.get("Transparency")).clamp(0.0, 1.0),
        reflectance: super::number(properties.get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(properties),
    })
}

/// What [`resolve`] hands back for `Scene::resolve_unions` to merge in.
#[derive(Default)]
pub(crate) struct Resolution {
    /// Additive-only stand-ins for every union whose boolean failed.
    pub(crate) parts: Vec<Part>,
    /// Every union that resolved either way, whose fallback box must hide.
    pub(crate) hidden: HashSet<Ref>,
    /// One computed mesh per asset, shared by every instance of it.
    pub(crate) meshes: HashMap<AssetRef, rbx_mesh::Mesh>,
    pub(crate) instances: Vec<ResolvedInstance>,
}

/// One asset's decoded tree and, when the boolean succeeded, its mesh —
/// computed once however many instances share the asset.
struct Evaluated {
    tree: tree::Node,
    mesh: Option<rbx_mesh::Mesh>,
}

/// Resolves a plan against downloaded asset bytes.
///
/// Must run before [`super::Scene::resolve_materials`]: it is the one place
/// new [`Catalog`] slots for a union's fallback parts get discovered, and
/// `resolve_materials` is what re-reads every slot afterward.
///
/// The boolean can fail (too many polygons, an all-carved result, a tree that
/// did not parse); the first two fall back to the additive-only parts of the
/// old resolver and the last keeps the box. Never a hole-ridden mesh.
pub(crate) fn resolve(
    plan: &Plan,
    assets: HashMap<AssetRef, Vec<u8>>,
    database: &ReflectionDatabase,
    materials: &mut Catalog,
) -> Resolution {
    let evaluated = evaluate_all(plan, &assets, database);
    let mut resolution = Resolution::default();

    for entry in &plan.entries {
        let Some(Some(Evaluated { tree, mesh })) = evaluated.get(&entry.asset) else {
            continue;
        };
        resolution.hidden.insert(entry.referent);

        if mesh.is_none() {
            tree.fallback_parts(
                entry.placement(),
                entry.referent,
                database,
                materials,
                &mut resolution.parts,
            );
            continue;
        }
        // Invisible either way; hidden above so the box never reappears.
        if entry.alpha <= 0.0 {
            continue;
        }
        let color = entry
            .color
            .or_else(|| csg::largest_additive_color(tree))
            .unwrap_or(super::FALLBACK_COLOR);
        resolution.instances.push(
            entry.instance(color.map(|channel| super::srgb_to_linear(f32::from(channel) / 255.0))),
        );
    }

    resolution.meshes = evaluated
        .into_iter()
        .filter_map(|(asset, evaluated)| Some((asset, evaluated?.mesh?)))
        .collect();
    resolution
}

/// Decodes and carves one asset. `None` only when the bytes did not parse;
/// a parsed tree whose boolean failed keeps `mesh: None` for the fallback.
fn evaluate(bytes: &[u8], database: &ReflectionDatabase) -> Option<Evaluated> {
    let tree = tree::parse(bytes, database)?;
    let mesh = csg::evaluate(&tree).ok().map(|solid| solid.to_mesh());
    Some(Evaluated { tree, mesh })
}

/// Runs [`evaluate`] for every distinct, downloaded asset `plan` needs,
/// across a bounded worker pool — same `thread::scope` plus atomic work-list
/// index shape as `crate::assets::load_with`'s download pool, just with the
/// BSP boolean itself as the unit of work instead of a network fetch.
///
/// This is the one CPU-heavy step in resolving a plan: each asset's boolean
/// is a from-scratch BSP tree build, completely independent of every other
/// asset's, so a place with dozens of legacy unions can spread that cost
/// across cores instead of paying for it back to back on the caller's own
/// thread. Several entries can share an asset (a builder copy-pasting the
/// same rock), so this dedupes by [`AssetRef`] first — the whole point is
/// never redoing the same boolean twice, in parallel or not.
fn evaluate_all(
    plan: &Plan,
    assets: &HashMap<AssetRef, Vec<u8>>,
    database: &ReflectionDatabase,
) -> HashMap<AssetRef, Option<Evaluated>> {
    let mut seen = HashSet::new();
    let unique: Vec<&AssetRef> = plan
        .entries
        .iter()
        .map(|entry| &entry.asset)
        .filter(|asset| assets.contains_key(*asset) && seen.insert((*asset).clone()))
        .collect();
    if unique.is_empty() {
        return HashMap::new();
    }

    let next = AtomicUsize::new(0);
    let results = Mutex::new(HashMap::with_capacity(unique.len()));
    let workers = MAX_CSG_WORKERS
        .min(std::thread::available_parallelism().map_or(1, |n| n.get()))
        .min(unique.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(&asset) = unique.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let evaluated = evaluate(&assets[asset], database);
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(asset.clone(), evaluated);
                }
            });
        }
    });

    results.into_inner().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "union/tests.rs"]
mod tests;
