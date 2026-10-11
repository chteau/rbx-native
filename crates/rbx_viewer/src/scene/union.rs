//! `UnionOperation`/`NegateOperation` handling: recovers the original,
//! pre-CSG parts and recomputes the boolean over them (`csg`), instead of
//! drawing the union as a plain box forever. The parts come from the union's
//! own `ChildData2` (`ChildData` in older files) where it carries them, as
//! every union Studio writes today does, or else from the
//! `PartOperationAsset` its legacy `AssetId` names. Roblox's own baked
//! `MeshData`/`MeshData2` is read where neither holds a tree, or where the
//! boolean cannot carve the tree there is (`baked`).
//!
//! Two-phase like `filemesh`: [`plan`] walks the DOM for every operation, and
//! [`resolve`] joins that plan against downloaded asset bytes once they exist
//! (an inline tree is its own bytes). No network access happens here — see
//! `crate::assets` for downloading.

mod baked;
mod csg;
mod legacy;
mod patch;
mod tree;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::textures::asset_uri;

use super::material::{Catalog, Slot};
use super::{Part, ResolvedInstance};

pub(crate) use csg::unit_mesh;
pub(super) use patch::replan;

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
    /// The union's `AssetId`, or the [`inline_key`] of the tree it carries.
    asset: AssetRef,
    inline: bool,
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
            isolated_out: false,
        }
    }
}

/// Every legacy union/negate in a DOM, extracted once and reused both to list
/// the assets a caller must download and to resolve them afterward.
#[derive(Default)]
pub(crate) struct Plan {
    entries: Vec<Entry>,
    /// The operation documents unions carry inline, by their [`inline_key`]:
    /// resolved like downloaded assets, with nothing to download.
    inline: tree::Assets,
    /// The `MeshData` a union carrying its tree inline also carries, by the
    /// same key: what it is drawn as if the boolean fails. A union behind an
    /// `AssetId` finds its bake in that asset instead.
    bakes: tree::Assets,
}

impl Plan {
    /// One `(referent, asset)` pair per union whose tree is behind its
    /// `AssetId`, in first-seen order; a union carrying its tree inline has
    /// nothing to download (the assets such a tree names deeper down are
    /// [`missing`]'s to find). Several unions can share the same `AssetId` (a
    /// builder copy-pasting a rock, say); callers should dedupe before
    /// downloading, same as `filemesh`'s `mesh_refs`/`texture_refs`.
    pub(crate) fn assets(&self) -> Vec<(Ref, AssetRef)> {
        self.entries
            .iter()
            .filter(|entry| !entry.inline)
            .map(|entry| (entry.referent, entry.asset.clone()))
            .collect()
    }
}

/// Walks a DOM for every `PartOperation` (`UnionOperation`, `NegateOperation`)
/// whose pre-CSG tree is still reachable: inline in `ChildData`/`ChildData2`,
/// or in the legacy `AssetId` asset it was baked into.
///
/// Workspace-scoped, same as `Scene::from_dom`'s own part build: a union
/// staged outside `Workspace` never draws, so there is nothing to plan for it.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase, materials: &mut Catalog) -> Plan {
    let mut plan = Plan::default();
    for referent in super::workspace_descendants(dom, database)
        .filter(|&referent| super::is_drawable(dom, database, referent))
    {
        let Some(entry) = from_operation(dom, database, referent, materials) else {
            continue;
        };
        if entry.inline && !plan.inline.contains_key(&entry.asset) {
            if let Some(raw) = dom
                .get(referent)
                .and_then(|i| inline_document(i.properties()))
            {
                plan.inline.insert(entry.asset.clone(), raw.to_vec());
            }
            if let Some(blob) = dom
                .get(referent)
                .filter(|i| tree::child_data(i.properties()).is_some())
                .and_then(|i| baked::mesh_data(i.properties()))
            {
                plan.bakes.insert(entry.asset.clone(), blob.to_vec());
            }
        }
        plan.entries.push(entry);
    }
    plan
}

/// The key an inline operation document is evaluated and drawn under: a
/// digest of the bytes, so every copy of one union shares one boolean, and an
/// edit that leaves them alone finds it already carved. `rbxthumb` is never
/// fetched (see `rbx_assets::resolver`), the same reason
/// `renderer::gui::viewport` keys its baked frames that way.
fn inline_key(raw: &[u8]) -> AssetRef {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    raw.hash(&mut hasher);
    AssetRef::Thumb(format!("csg-inline/{:016x}-{}", hasher.finish(), raw.len()))
}

/// The bytes a union carries for itself: its operation tree, or — only
/// when it has neither a tree nor an `AssetId` to find one behind — the
/// mesh Studio baked for it.
fn inline_document(properties: &std::collections::BTreeMap<String, Variant>) -> Option<&[u8]> {
    tree::child_data(properties).or_else(|| {
        let linked = properties
            .get("AssetId")
            .and_then(asset_uri)
            .is_some_and(|uri| !uri.is_empty());
        if linked {
            return None;
        }
        baked::mesh_data(properties)
    })
}

/// Whether `referent` is a union with no geometry anywhere: no inline tree,
/// no asset, and no baked mesh either. Studio writes these with a
/// `TriangleCount` of 0, and draws nothing for them, so neither does this.
pub(crate) fn is_empty(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    let Some(instance) = dom.get(referent) else {
        return false;
    };
    if !database.is_subclass_of(instance.class(), PART_OPERATION) {
        return false;
    }
    let properties = instance.properties();
    let filled = |key: &str| match properties.get(key) {
        Some(Variant::Unknown { raw, .. }) => !raw.is_empty(),
        Some(Variant::String(text)) => !text.is_empty(),
        Some(Variant::Content(rbx_dom::Content::Uri(uri))) => !uri.is_empty(),
        _ => false,
    };
    ![
        "AssetId",
        "ChildData",
        "ChildData2",
        "MeshData",
        "MeshData2",
    ]
    .into_iter()
    .any(filled)
}

fn from_operation(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    let (asset, inline, cframe, size, initial_size) = frame(dom, database, referent)?;
    let properties = dom.get(referent)?.properties();
    let use_part_color = matches!(properties.get("UsePartColor"), Some(Variant::Bool(true)));
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) if use_part_color => Some([r, g, b]),
        _ => None,
    };

    Some(Entry {
        referent,
        asset,
        inline,
        cframe,
        size,
        initial_size,
        material: materials.slot_for(properties, database),
        color,
        alpha: 1.0 - super::number(properties.get("Transparency")).clamp(0.0, 1.0),
        reflectance: super::number(properties.get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(properties),
    })
}

/// A union's key (its asset, or its inline tree's [`inline_key`] — the
/// inline tree winning, since it needs no download), whether it is inline,
/// and its world `CFrame`, `size` and `InitialSize`: all its computed mesh
/// needs to be placed where it is drawn.
fn frame(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
) -> Option<(AssetRef, bool, Mat4, Vec3, Vec3)> {
    let instance = dom.get(referent)?;
    if !database.is_subclass_of(instance.class(), PART_OPERATION) {
        return None;
    }
    let properties = instance.properties();

    let linked = properties
        .get("AssetId")
        .and_then(asset_uri)
        .and_then(|uri| AssetRef::parse(uri).ok())
        .filter(|asset| *asset != AssetRef::Empty);
    let (asset, inline) = match (inline_document(properties), linked) {
        (Some(raw), _) => (inline_key(raw), true),
        (None, Some(asset)) => (asset, false),
        (None, None) => return None,
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
    Some((
        asset,
        inline,
        super::cframe_matrix(cframe),
        size,
        initial_size,
    ))
}

/// The asset a legacy union's computed boolean is keyed by, and the unit-mesh
/// to world transform it is drawn with ([`Entry::placement`]), read off the
/// DOM as it stands now. Whether that mesh exists is the caller's lookup: a
/// union whose boolean failed or has not downloaded has none.
pub(crate) fn fit(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
) -> Option<(AssetRef, Mat4)> {
    let (asset, _, cframe, size, initial_size) = frame(dom, database, referent)?;
    Some((asset, cframe * Mat4::from_scale(size / initial_size)))
}

/// What [`resolve`] hands back for `Scene::resolve_unions` to merge in.
#[derive(Default)]
pub(crate) struct Resolution {
    /// Additive-only stand-ins for every union whose boolean failed.
    pub(crate) parts: Vec<Part>,
    /// Every union that resolved either way, whose fallback box must hide.
    pub(crate) hidden: HashSet<Ref>,
    /// One computed mesh per asset, shared by every instance of it.
    pub(crate) meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
    pub(crate) instances: Vec<ResolvedInstance>,
}

/// Every [`Resolution`] so far, merged.
///
/// A place's unions do not all resolve at the same moment any more: a
/// streaming load hands [`resolve`] each asset the tick it lands, and what it
/// contributed has to survive the next `Scene::resolve_file_meshes`, which
/// rebuilds the resolved set from the file mesh plan alone. So the meshes and
/// instances are kept here to be laid back on top of it, while the recovered
/// parts — which are appended to the scene's own parts once and never
/// rebuilt — are handed straight over through `fresh_parts`.
#[derive(Default)]
pub(crate) struct Merged {
    pub(crate) hidden: HashSet<Ref>,
    pub(crate) meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
    pub(crate) instances: Vec<ResolvedInstance>,
    /// The parts of the latest [`Merged::absorb`] alone, for the caller to
    /// take. Empty at every other moment.
    pub(crate) fresh_parts: Vec<Part>,
    /// Every union referent merged in so far, whichever way it resolved. What
    /// makes [`Merged::absorb`] idempotent — see its doc comment.
    absorbed: HashSet<Ref>,
}

impl Merged {
    /// Merges one [`resolve`] in, ignoring whatever it says about a union
    /// already merged.
    ///
    /// [`resolve`] answers for every union whose asset it can evaluate, and
    /// a streaming load calls it once a tick: once an asset has been carved
    /// it is answered for on every later tick too, out of the evaluations
    /// rather than its bytes. The instances are appended, and a boolean that
    /// failed recovers *several* additive parts under the union's own
    /// referent, so absorbing the same union twice would draw it twice.
    pub(crate) fn absorb(&mut self, resolution: Resolution) {
        self.hidden.extend(resolution.hidden);
        self.meshes.extend(resolution.meshes);
        let fresh: HashSet<Ref> = resolution
            .instances
            .iter()
            .map(|instance| instance.referent)
            .chain(resolution.parts.iter().map(|part| part.referent()))
            .filter(|referent| !self.absorbed.contains(referent))
            .collect();
        self.absorbed.extend(fresh.iter().copied());
        self.instances.extend(
            resolution
                .instances
                .into_iter()
                .filter(|instance| fresh.contains(&instance.referent)),
        );
        self.fresh_parts = resolution
            .parts
            .into_iter()
            .filter(|part| fresh.contains(&part.referent()))
            .collect();
    }
}

/// One asset's decoded tree and, when the boolean succeeded, its mesh —
/// computed once however many instances share the asset.
pub(super) struct Evaluated {
    tree: tree::Node,
    mesh: Option<Arc<rbx_mesh::Mesh>>,
    /// The colour a mesh decoded from `MeshData` was mostly baked in; `None`
    /// for one carved from its tree, whose leaves say instead.
    baked_color: Option<[u8; 3]>,
}

/// Every asset's [`Evaluated`] so far — `None` where its bytes did not
/// parse — kept across scene rebuilds by whoever owns the place (see
/// `load::Resident`). A boolean is a function of the asset's bytes alone,
/// never of the union placed in the scene, so once carved it is carved for
/// good: a reload re-plans every union from the DOM and finds every one of
/// its assets already here.
#[derive(Default)]
pub(crate) struct Evaluations {
    known: HashMap<AssetRef, Option<Arc<Evaluated>>>,
}

impl Evaluations {
    /// Whether this place has already tried to carve `asset` at all —
    /// carved it, or found its bytes unparseable. [`resolve`] then needs
    /// none of those bytes, so a reload can skip fetching them (see
    /// `load::resolve_unions`), and an edit pointing a union at the asset
    /// is answered here rather than by a reload (see
    /// `Scene::resync_union`): a `true` with no [`Evaluations::of`] is a
    /// permanent answer, and the same one a reload would come to.
    pub(crate) fn is_known(&self, asset: &AssetRef) -> bool {
        self.known.contains_key(asset)
    }
}

/// Resolves a plan against downloaded asset bytes.
///
/// Must run before [`super::Scene::resolve_materials`]: it is the one place
/// new [`Catalog`] slots for a union's fallback parts get discovered, and
/// `resolve_materials` is what re-reads every slot afterward.
///
/// The boolean can fail (too many polygons, an all-carved result, a tree that
/// did not parse); the first two draw the mesh Studio baked for the union
/// where it has one, and the additive-only parts of the old resolver where
/// not, and the last keeps the box. Never a hole-ridden mesh.
///
/// `assets` need only hold the bytes of what `evaluations` has not seen: an
/// asset it knows is resolved from what it carved before, bytes or not.
pub(crate) fn resolve(
    plan: &Plan,
    assets: HashMap<AssetRef, Vec<u8>>,
    database: &ReflectionDatabase,
    materials: &mut Catalog,
    evaluations: &mut Evaluations,
) -> Resolution {
    let mut assets = assets;
    for (key, raw) in &plan.inline {
        if !evaluations.is_known(key) {
            assets.entry(key.clone()).or_insert_with(|| raw.clone());
        }
    }
    let evaluated = evaluate_all(plan, &assets, database, evaluations);
    let mut resolution = Resolution::default();

    for entry in &plan.entries {
        let Some(Some(evaluated)) = evaluated.get(&entry.asset) else {
            continue;
        };
        let Evaluated { tree, mesh, .. } = evaluated.as_ref();
        resolution.hidden.insert(entry.referent);

        if mesh.is_none() {
            resolution.parts.extend(tree.pieces(
                entry.placement(),
                entry.referent,
                database,
                materials,
            ));
            continue;
        }
        // Invisible either way; hidden above so the box never reappears.
        if entry.alpha <= 0.0 {
            continue;
        }
        resolution
            .instances
            .push(entry.instance(entry.carved_color(evaluated)));
    }

    resolution.meshes = evaluated
        .into_iter()
        .filter_map(|(asset, evaluated)| Some((asset, evaluated?.mesh.clone()?)))
        .collect();
    resolution
}

/// Decodes and carves one asset, `assets` holding the nested ones its tree
/// points at. The outer `None` is "not yet": a nested asset has not arrived,
/// and carving now would remember a box in its place for good. The inner
/// `None` is "never": the bytes did not parse. A parsed tree whose boolean
/// failed draws the mesh Studio baked for it, read out of `fallback`, or
/// keeps `mesh: None` for the pieces where there is none.
fn evaluate(
    bytes: &[u8],
    database: &ReflectionDatabase,
    assets: &tree::Assets,
    bake: Option<Vec3>,
    fallback: Option<&[u8]>,
) -> Option<Option<Evaluated>> {
    let Some(parsed) = legacy::parse_as_baked(bytes, database, assets, bake) else {
        // No tree to carve: Studio's own bake is all there is.
        return Some(baked::of_bytes(bytes).map(|baked| Evaluated {
            tree: tree::Node::Operation {
                negate: false,
                children: Vec::new(),
            },
            mesh: Some(Arc::new(baked.mesh)),
            baked_color: baked.color,
        }));
    };
    if !parsed.missing.is_empty() {
        return None;
    }
    let tree = parsed.root;
    if let Ok(solid) = csg::evaluate(&tree, bake) {
        return Some(Some(Evaluated {
            tree,
            mesh: Some(Arc::new(solid.to_mesh())),
            baked_color: None,
        }));
    }
    let baked = fallback.and_then(baked::of_bytes);
    Some(Some(Evaluated {
        tree,
        baked_color: baked.as_ref().and_then(|baked| baked.color),
        mesh: baked.map(|baked| Arc::new(baked.mesh)),
    }))
}

/// The nested union assets that the trees in `assets`, and the inline trees
/// of `plan` not yet carved, point at and `assets` does not hold yet — what
/// a loader fetches next, before [`resolve`] can carve those trees.
pub(crate) fn missing(
    plan: &Plan,
    assets: &tree::Assets,
    database: &ReflectionDatabase,
    evaluations: &Evaluations,
) -> Vec<AssetRef> {
    let inline = plan
        .inline
        .iter()
        .filter(|(key, _)| !evaluations.is_known(key))
        .map(|(_, raw)| raw);
    let mut missing = Vec::new();
    for bytes in assets.values().chain(inline) {
        for asset in tree::parse(bytes, database, assets)
            .map(|parsed| parsed.missing)
            .unwrap_or_default()
        {
            if !missing.contains(&asset) {
                missing.push(asset);
            }
        }
    }
    missing
}

/// Every distinct asset `plan` needs that is either already known or
/// downloaded, evaluated: out of `evaluations` where an earlier scene already
/// carved it, and through [`evaluate`] across a bounded worker pool where
/// not — same `thread::scope` plus atomic work-list index shape as
/// `crate::assets::load_with`'s download pool, just with the BSP boolean
/// itself as the unit of work instead of a network fetch. Whatever is carved
/// here is remembered in `evaluations`.
///
/// This is the one CPU-heavy step in resolving a plan: each asset's boolean
/// is a from-scratch BSP tree build, completely independent of every other
/// asset's, so a place with dozens of legacy unions can spread that cost
/// across cores instead of paying for it back to back on the caller's own
/// thread. Several entries can share an asset (a builder copy-pasting the
/// same rock), so this dedupes by [`AssetRef`] first — the whole point is
/// never redoing the same boolean twice, in parallel or not, and a reload
/// is the same boolean again.
fn evaluate_all(
    plan: &Plan,
    assets: &HashMap<AssetRef, Vec<u8>>,
    database: &ReflectionDatabase,
    evaluations: &mut Evaluations,
) -> HashMap<AssetRef, Option<Arc<Evaluated>>> {
    let mut seen = HashSet::new();
    // The first union's `InitialSize` stands for every one sharing the
    // asset: it is the extent of the very bake the asset holds.
    let wanted: Vec<(&AssetRef, Vec3)> = plan
        .entries
        .iter()
        .map(|entry| (&entry.asset, entry.initial_size))
        .filter(|(asset, _)| {
            (evaluations.known.contains_key(*asset) || assets.contains_key(*asset))
                && seen.insert((*asset).clone())
        })
        .collect();
    let unique: Vec<(&AssetRef, Vec3)> = wanted
        .iter()
        .copied()
        .filter(|(asset, _)| !evaluations.known.contains_key(*asset))
        .collect();

    if !unique.is_empty() {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(HashMap::with_capacity(unique.len()));
        let workers = MAX_CSG_WORKERS
            .min(std::thread::available_parallelism().map_or(1, |n| n.get()))
            .min(unique.len());

        let work = || {
            while let Some(&(asset, bake)) = unique.get(next.fetch_add(1, Ordering::Relaxed)) {
                let bytes = &assets[asset];
                // An inline tree's own bytes hold no bake of the union (only,
                // maybe, of a union nested in it); an asset's root does.
                let fallback = match plan.inline.contains_key(asset) {
                    true => plan.bakes.get(asset).map(Vec::as_slice),
                    false => Some(bytes.as_slice()),
                };
                if let Some(evaluated) = evaluate(bytes, database, assets, Some(bake), fallback) {
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(asset.clone(), evaluated.map(Arc::new));
                }
            }
        };
        // One worker is the calling thread itself: no thread to spawn, which
        // is also the only way this runs at all in a browser (no threads on
        // wasm, where `available_parallelism` answers an error, so one).
        if workers <= 1 {
            work();
        } else {
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(work);
                }
            });
        }
        evaluations
            .known
            .extend(results.into_inner().unwrap_or_else(|e| e.into_inner()));
    }

    wanted
        .into_iter()
        .filter_map(|(asset, _)| Some((asset.clone(), evaluations.known.get(asset)?.clone())))
        .collect()
}

#[cfg(test)]
#[path = "union/tests_support.rs"]
pub(in crate::scene) mod tests_support;

#[cfg(test)]
#[path = "union/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "union/inline_tests.rs"]
mod inline_tests;

#[cfg(test)]
#[path = "union/survey.rs"]
mod survey;
