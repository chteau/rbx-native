//! Pure DOM extraction for `MeshPart` and `Part`-with-`SpecialMesh`(FileMesh)
//! instances: which mesh/texture assets they need, and how to place the
//! downloaded geometry once it exists. No network or GPU access happens here,
//! see `crate::assets` for downloading and `crate::renderer::filemesh` for the
//! GPU side.

mod appearance;
mod clothing;
mod fit;
mod makeup;
pub(crate) mod skinned;
mod wrap;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::textures::asset_uri;
use crate::textures::clothing::Limb;

pub(crate) use appearance::{of as appearance_of, AlphaMode, Appearance};
pub(crate) use fit::{
    mesh_part as mesh_part_asset, of as fit_of, special_mesh as special_mesh_asset, Fit,
};
pub(crate) use wrap::Hsr;

use super::material::{Catalog, Kind, Slot};
use super::shape::{FILE_MESH, HEAD_MESH};

const MESH_PART: &str = "MeshPart";

/// One file-mesh-backed instance found in the DOM, before its assets exist.
#[derive(Clone)]
pub(super) struct Entry {
    material: Slot,
    /// The drawn `Part` this entry stands in for: the `MeshPart` itself, or
    /// the parent of a `SpecialMesh` child. Used to find and hide that part's
    /// fallback box once real geometry resolves, see `Scene::resolve_file_meshes`.
    referent: Ref,
    mesh: AssetRef,
    /// The mesh's own `TextureID`, and `None` whenever `appearance` is set: a
    /// `SurfaceAppearance` replaces it outright rather than layering over it.
    texture: Option<AssetRef>,
    appearance: Option<Appearance>,
    fit: Fit,
    /// The part's colour, what an untextured mesh is drawn in.
    color: [f32; 3],
    /// What a textured mesh's image is multiplied by: white for a MeshPart,
    /// a SpecialMesh's `VertexColor`. Neither takes the part's colour: an
    /// accessory handle left at the default grey draws its texture as is
    /// (next to the real client, a white floatie at 210 drew at 133 when
    /// this was the part's colour).
    tint: [f32; 3],
    /// What shows where the image is see-through.
    backing: Backing,
    alpha: f32,
    reflectance: f32,
    casts_shadow: bool,
    /// The classic clothing an R15 limb section wears, cut into a derived
    /// mesh and image once everything it needs has arrived.
    dressing: Option<Box<clothing::Dressing>>,
    /// Face makeup, laid into the colour map in the head's own UVs.
    makeup: Vec<makeup::Makeup>,
    /// A layered garment's cages, to fit it to the body it is worn on.
    wrap: Option<wrap::Wrap>,
    /// The `Model` a `MeshPart` sits in, where a skinned mesh finds the
    /// parts its bones are named after (see [`skinned`]).
    model: Option<Ref>,
}

/// Every file-mesh instance in a DOM, extracted once and reused both to list
/// the assets a caller must download and to resolve them afterward.
#[derive(Default)]
pub(crate) struct Plan {
    entries: Vec<Entry>,
    /// Rest and current frames of every part a skinned mesh could bind to.
    pub(crate) poses: skinned::Poses,
    /// Mesh parts whose model's parts changed since their rig was bound:
    /// re-patched at the end of the batch (see `Scene::take_skin_rebinds`).
    pub(crate) rebinds: HashSet<Ref>,
    /// Derived meshes rebuilt by a patch, whose GPU copy is stale.
    pub(crate) rebound: Vec<AssetRef>,
    /// The `HiddenSurfaceRemovalAsset`s read so far, see `wrap::hsr`.
    pub(crate) surfaces: HashMap<AssetRef, Arc<Hsr>>,
}

/// One instance ready for the renderer: which downloaded mesh/texture it
/// draws, its model matrix, and its tint.
///
/// `Clone`: a union's instances outlive the resolved set they sit in, which a
/// streaming load rebuilds from the file mesh plan every time more meshes
/// land, see `scene::union::Merged`.
#[derive(Clone)]
pub(crate) struct ResolvedInstance {
    /// The part this instance draws in place of (see [`Entry::referent`]), so
    /// the renderer's per-instance patch maps and `Scene::resync_part`
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
    /// `BasePart.CastShadow` of the part this mesh stands for, see
    /// [`super::Part::casts_shadow`].
    pub(crate) casts_shadow: bool,
    /// The part's own [`super::Part::is_drawn`] isolation flag, carried
    /// over because a mesh part draws through this instance and not its box
    /// see `scene::isolation`.
    pub(crate) isolated_out: bool,
}

impl ResolvedInstance {
    /// Whether any pass paints this instance: everything a scene resolved
    /// does, except what an isolation hides.
    pub(crate) fn is_drawn(&self) -> bool {
        !self.isolated_out
    }
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
    /// rather than a copy of every mesh in the place, and so the decoded
    /// mesh kept across reloads (see `load::Resident`) is that same one.
    pub(crate) meshes: HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    /// Behind `Arc`s for the second of those reasons.
    pub(crate) images: HashMap<AssetRef, Arc<Image>>,
    /// Every distinct `SurfaceAppearance` the scene resolved, deduplicated:
    /// a character's dozen limbs usually share one map set.
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) instances: Vec<ResolvedInstance>,
    /// Where each referent's instance sits in `instances`, for an edit to
    /// find without a scan; kept in step by [`Resolved::push`] and
    /// [`Resolved::remove`], the only two places the list changes length.
    /// A referent with two entries (two `SpecialMesh` children on one part)
    /// answers with its first, the same one a scan would find.
    slot_of: HashMap<Ref, usize>,
    /// Every skinned instance, by the part it draws, whose bones bind to
    /// parts of its model (see [`skinned`]); it draws through
    /// `skinned::key(referent, mesh)` in `meshes`.
    pub(crate) rigs: HashMap<Ref, skinned::Rig>,
    /// Whether the renderer deforms the rigs on the GPU: `meshes` then holds
    /// each rig's rest mesh under its key, and [`skinned::Rig::posed`]
    /// answers for the pose on the CPU. Off, `meshes` holds the posed copy.
    pub(crate) skins_on_gpu: bool,
}

impl Resolved {
    pub(crate) fn slot_of(&self, referent: Ref) -> Option<usize> {
        self.slot_of.get(&referent).copied()
    }

    /// Puts `instances` in place of whatever was drawn for their referents
    /// (the unions' own, put back after every file mesh pass), indexed like
    /// every other instance so an edit or a delete finds them.
    pub(crate) fn replace(&mut self, instances: &[ResolvedInstance]) {
        let referents: HashSet<Ref> = instances.iter().map(|i| i.referent).collect();
        self.instances.retain(|i| !referents.contains(&i.referent));
        self.instances.extend(instances.iter().cloned());
        self.slot_of.clear();
        for (index, instance) in self.instances.iter().enumerate() {
            self.slot_of.entry(instance.referent).or_insert(index);
        }
    }

    /// Appends `instance`, answering with the index it got.
    pub(crate) fn push(&mut self, instance: ResolvedInstance) -> usize {
        let index = self.instances.len();
        self.slot_of.entry(instance.referent).or_insert(index);
        self.instances.push(instance);
        index
    }

    /// Takes `referent`'s instance out. Order only ever mattered to a full
    /// build's batch construction, the renderer finds instances by referent
    /// from there on, so the last one fills the hole.
    /// Takes `referent`'s rig, and the derived mesh it drew, out.
    pub(crate) fn drop_rig(&mut self, referent: Ref) {
        if let Some(rig) = self.rigs.remove(&referent) {
            self.meshes.remove(&rig.key);
        }
    }

    pub(crate) fn remove(&mut self, referent: Ref) {
        let Some(index) = self.slot_of.remove(&referent) else {
            return;
        };
        self.instances.swap_remove(index);
        if let Some(moved) = self.instances.get(index) {
            self.slot_of.insert(moved.referent, index);
        }
    }
}

impl Entry {
    /// The mesh this entry draws and every image it samples, its own
    /// `TextureID` and, where it has one, the four `SurfaceAppearance` maps.
    pub(super) fn assets(&self) -> (AssetRef, Vec<AssetRef>) {
        let images = self
            .texture
            .iter()
            .chain(self.appearance.iter().flat_map(|appearance| {
                appearance
                    .maps
                    .iter()
                    .flatten()
                    .chain(appearance.emissive_mask.iter())
            }))
            .chain(self.dressing.iter().flat_map(|dressing| dressing.images()))
            .chain(makeup::images(&self.makeup))
            .cloned()
            .collect();
        (self.mesh.clone(), images)
    }
}

impl Plan {
    /// Marks every mesh part in `models` for re-binding.
    pub(crate) fn rebind_models(&mut self, models: &[Ref]) {
        if models.is_empty() {
            return;
        }
        let parts = self
            .entries
            .iter()
            .filter(|entry| {
                let character = entry.wrap.as_ref().map(|wrap| wrap.character);
                entry
                    .model
                    .or(character)
                    .is_some_and(|model| models.contains(&model))
            })
            .map(|entry| entry.referent);
        self.rebinds.extend(parts);
    }

    /// Every distinct mesh asset this plan needs, in first-seen order.
    pub(crate) fn mesh_refs(&self) -> Vec<AssetRef> {
        dedup(self.entries.iter().flat_map(|entry| {
            std::iter::once(&entry.mesh).chain(entry.wrap.iter().flat_map(wrap::Wrap::meshes))
        }))
    }

    /// The `HiddenSurfaceRemovalAsset`s worn garments and their bodies name,
    /// for the faces to hide under the clothing.
    pub(crate) fn hsr_refs(&self) -> Vec<AssetRef> {
        dedup(
            self.entries
                .iter()
                .flat_map(|entry| entry.wrap.iter().flat_map(wrap::Wrap::hsr_assets)),
        )
    }

    /// Swaps one part's entry for `entry`, in the place the old one held, or
    /// drops it where the part is no longer file-mesh-backed at all.
    ///
    /// A plan is made once from the DOM and joined to assets again every time
    /// more of them land (see `load::Loaded::resolve`), long after the DOM is
    /// out of reach, so a single-instance edit that changes what an instance
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
        dedup(
            self.entries
                .iter()
                .flat_map(|entry| entry.assets().1)
                .collect::<Vec<_>>()
                .iter(),
        )
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
        .collect::<Vec<_>>();
    let poses = skinned::Poses::plan(dom, database, &entries);
    Plan {
        entries,
        poses,
        ..Plan::default()
    }
}

/// [`plan`] for the instances under `root` alone, wherever it sits: a
/// `ViewportFrame` draws its own meshes, out of the workspace.
pub(crate) fn plan_under(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    root: Ref,
    materials: &mut Catalog,
) -> Plan {
    let entries = super::descendants_of(dom, root)
        .filter(|&referent| super::is_drawable(dom, database, referent))
        .filter_map(|referent| {
            from_mesh_part(dom, database, referent, materials)
                .or_else(|| from_special_mesh_child(dom, database, referent, materials))
        })
        .collect::<Vec<_>>();
    let poses = skinned::Poses::plan(dom, database, &entries);
    Plan {
        entries,
        poses,
        ..Plan::default()
    }
}

/// Resolves a plan against downloaded assets.
///
/// Returns the renderer-ready instances alongside the set of referents that
/// got real geometry, so the fallback box of everything else can stay put.
pub(crate) fn resolve(
    plan: &Plan,
    meshes: HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    images: HashMap<AssetRef, Arc<Image>>,
) -> (Resolved, HashSet<Ref>) {
    let mut instances = Vec::new();
    let mut appearances: Vec<Appearance> = Vec::new();
    let mut hidden = HashSet::new();
    let mut rigs = HashMap::new();

    let mut meshes = meshes;
    let mut images = images;
    let dressed = clothing::derive(&plan.entries, &meshes, &images);
    let wrapped = wrap::derive(&plan.entries, &meshes, &plan.surfaces);

    for entry in &plan.entries {
        let Some(mesh) = meshes.get(&entry.mesh).cloned() else {
            continue;
        };
        // Hidden either way: a fully transparent MeshPart draws nothing at
        // all, and its fallback box reappearing would be worse than nothing.
        hidden.insert(entry.referent);
        if entry.alpha <= 0.0 {
            continue;
        }

        let mut texture = entry
            .texture
            .clone()
            .filter(|reference| images.contains_key(reference));
        let (mut mesh_key, mut color) = (entry.mesh.clone(), entry.color_for(texture.is_some()));
        if let Some(reference) = texture.clone() {
            texture = Some(backed(&reference, entry.backing, entry.color, &mut images));
        }
        if let Some(dressed) = dressed.get(&entry.referent) {
            let (key, derived) = &dressed.mesh;
            let (image_key, image) = &dressed.image;
            meshes.insert(key.clone(), derived.clone());
            images.insert(image_key.clone(), image.clone());
            (mesh_key, texture, color) = (key.clone(), Some(image_key.clone()), [1.0; 3]);
        }
        if let Some((key, derived)) = wrapped.garments.get(&entry.referent) {
            meshes.insert(key.clone(), derived.clone());
            mesh_key = key.clone();
            // The wrapped garment follows the character's pose through a
            // skin of its own or one the body lent it.
            if let Some(rig) = skinned::Rig::garment(entry, &mesh, derived, &plan.poses) {
                meshes.insert(rig.key.clone(), derived.clone());
                mesh_key = rig.key.clone();
                rigs.insert(entry.referent, rig);
            }
        }
        // A body part under layered clothing leaves out the faces it hides
        // (the dressed copy keeps the source's triangle order).
        let mut drawn = mesh.clone();
        if let Some(hidden) = wrapped.hidden.get(&entry.referent) {
            if let Some(source) = meshes.get(&mesh_key) {
                drawn = Arc::new(wrap::cull(source, hidden));
                // Wholly under the clothing: nothing of it is drawn.
                if drawn.indices.is_empty() {
                    continue;
                }
                mesh_key = wrap::hidden_key(entry.referent);
                meshes.insert(mesh_key.clone(), drawn.clone());
            }
        }
        // A skinned mesh drawn as itself (not dressed, not wrapped) draws
        // through a copy of its own, posed later, the very same `Arc` at
        // rest, so nothing is copied until a bone moves.
        if !dressed.contains_key(&entry.referent) && !wrapped.garments.contains_key(&entry.referent)
        {
            if let Some(rig) = skinned::Rig::of(entry, &drawn, &plan.poses) {
                meshes.insert(rig.key.clone(), drawn.clone());
                mesh_key = rig.key.clone();
                rigs.insert(entry.referent, rig);
            }
        }
        // Makeup is laid into the colour map the head would draw.
        let mut made_up = false;
        if let Some(key) = makeup::key(entry, &images) {
            if !images.contains_key(&key) {
                images.insert(key.clone(), Arc::new(makeup::composite(entry, &images)));
            }
            if entry.texture.is_none() {
                color = [1.0; 3];
            }
            (texture, made_up) = (Some(key), true);
        }
        // A dressed section is drawn from its composite, the appearance's
        // colour map already under the clothes in it.
        let appearance = entry
            .appearance
            .as_ref()
            .filter(|_| !dressed.contains_key(&entry.referent) && !made_up)
            .map(|planned| {
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
            mesh: mesh_key,
            material: entry.material,
            texture,
            appearance,
            model: entry.fit.transform(&mesh),
            color,
            alpha: entry.alpha,
            reflectance: entry.reflectance,
            casts_shadow: entry.casts_shadow,
            isolated_out: false,
        });
    }

    let mut slot_of = HashMap::new();
    for (index, instance) in instances.iter().enumerate() {
        slot_of.entry(instance.referent).or_insert(index);
    }
    (
        Resolved {
            meshes,
            images,
            appearances,
            instances,
            slot_of,
            rigs,
            skins_on_gpu: false,
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
    let texture = crate::textures::content_property(properties, "TextureID")
        .and_then(parsed_asset_ref)
        .or_else(|| {
            properties
                .get("TextureContent")
                .and_then(crate::textures::editable_version_key)
        })
        .filter(|_| appearance.is_none());
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => super::FALLBACK_COLOR,
    };

    let mut material = materials.slot_for(properties, database);
    // Real Studio draws a textured Neon MeshPart as a SmoothPlastic one,
    // lit and with no glow, opaque or see-through alike; only an
    // untextured one is neon.
    if texture.is_some() && material.kind == Kind::Neon {
        let mut plain = properties.clone();
        if let Some(value) = database.enum_value("Material", "SmoothPlastic") {
            plain.insert("Material".to_owned(), Variant::Enum(value));
        }
        material = materials.slot_for(&plain, database);
    }
    let alpha = super::alpha(properties, material.kind);

    Some(Entry {
        referent,
        material,
        mesh,
        texture,
        appearance,
        fit,
        color: color.map(|channel| super::srgb_to_linear(f32::from(channel) / 255.0)),
        tint: [1.0; 3],
        backing: Backing::of_mesh_part(dom, database, referent, alpha),
        alpha,
        reflectance: super::number(properties.get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(properties),
        dressing: clothing::dressing(dom, referent).map(Box::new),
        makeup: makeup::of(dom, instance),
        wrap: wrap::of(dom, database, referent),
        model: skinned::model_of(dom, database, referent),
    })
}

/// A plain `Part` wearing a `SpecialMesh` FileMesh or Head child: the child
/// replaces the part's geometry outright, same precedence as
/// `scene::shape::resolve`. Every other `MeshType` is procedural, built in
/// `shape::resolve`, and is left alone.
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
    let texture =
        crate::textures::content_property(properties, "TextureId").and_then(parsed_asset_ref);
    // "Changes the hue of a mesh's texture" (creator-docs `VertexColor`), and
    // only that: Studio leaves an untextured mesh the part's own colour.
    let vertex_color = vector3(properties.get("VertexColor"), Vec3::ONE).to_array();
    let color = part_color.map(|channel| super::srgb_to_linear(f32::from(channel) / 255.0));
    let tint = vertex_color.map(super::srgb_to_linear);

    // The part's material, not the mesh's: a SpecialMesh has none.
    let material = materials.slot_for(part.properties(), database);

    Some(Entry {
        referent,
        material,
        mesh,
        texture,
        // `SurfaceAppearance` is a MeshPart child; a SpecialMesh never has one.
        appearance: None,
        fit,
        color,
        tint,
        backing: Backing::Image,
        // A SpecialMesh has no Transparency of its own: the part it hangs under
        // owns both properties.
        alpha: super::alpha(part.properties(), material.kind),
        reflectance: super::number(part.properties().get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: super::casts_shadow(part.properties()),
        dressing: None,
        makeup: makeup::of(dom, part),
        wrap: None,
        model: None,
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
/// What a textured mesh shows where its image is see-through, measured in
/// Studio 0.748 with the same images on MeshParts in and out of rigs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backing {
    /// The image's own alpha is kept: a SpecialMesh, and a see-through
    /// MeshPart, whose texels with no alpha vanish once Transparency is
    /// above 0.
    Image,
    /// The part's colour: an opaque R15 body part, the way a Rig Builder
    /// rig's head draws its face over the grey skin.
    Color,
    /// Nothing: any other opaque MeshPart draws its image's colour and drops
    /// its alpha, so where the image is empty (black, as Roblox serves it)
    /// it shows black, whatever the part's colour.
    Solid,
}

impl Backing {
    fn of_mesh_part(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        part: Ref,
        alpha: f32,
    ) -> Backing {
        if alpha < 1.0 {
            Backing::Image
        } else if is_r15_body_part(dom, database, part) {
            Backing::Color
        } else {
            Backing::Solid
        }
    }
}

/// Whether `part` is a section of an R15 character: named after one, in a
/// model whose Humanoid is R15. Studio only lays a body part's image over
/// its colour there; renamed, or under an R6 Humanoid, the same head shows
/// its image's own colour.
fn is_r15_body_part(dom: &WeakDom, database: &ReflectionDatabase, part: Ref) -> bool {
    const R15: u32 = 1;
    let Some(name) = dom.get(part).map(|p| p.name()) else {
        return false;
    };
    (name == "Head" || Limb::r15(name).is_some())
        && dom
            .parent(part)
            .and_then(|model| dom.get(model))
            .is_some_and(|model| {
                model
                    .children()
                    .iter()
                    .filter_map(|c| dom.get(*c))
                    .any(|child| {
                        child.class() == "Humanoid"
                            && matches!(
                                database.stored_or_default(child, "RigType"),
                                Some((_, Variant::Enum(R15)))
                            )
                    })
            })
}

/// What [`backed`] keeps `texture` under for `backing`.
fn backed_key(texture: &AssetRef, backing: Backing, color: [f32; 3]) -> AssetRef {
    match backing {
        Backing::Image => texture.clone(),
        Backing::Color => AssetRef::Thumb(format!("over/{texture:?}/{color:?}")),
        Backing::Solid => AssetRef::Thumb(format!("solid/{texture:?}")),
    }
}

/// `texture` with what shows through it filled in, opaque: the key it is
/// kept under (the image itself where it has no see-through texel, or where
/// its alpha is kept).
fn backed(
    texture: &AssetRef,
    backing: Backing,
    color: [f32; 3],
    images: &mut HashMap<AssetRef, Arc<Image>>,
) -> AssetRef {
    let key = backed_key(texture, backing, color);
    if !images.contains_key(&key) {
        let source = images[texture].clone();
        let image = if source
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == u8::MAX)
        {
            source
        } else {
            let under = color.map(|c| super::linear_to_srgb(c) * 255.0);
            let pixels = source
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = match backing {
                        Backing::Solid => 1.0,
                        _ => f32::from(p[3]) / 255.0,
                    };
                    let mix = |i: usize| (f32::from(p[i]) * a + under[i] * (1.0 - a)).round() as u8;
                    [mix(0), mix(1), mix(2), u8::MAX]
                })
                .collect();
            Arc::new(Image {
                width: source.width,
                height: source.height,
                pixels,
            })
        };
        images.insert(key.clone(), image);
    }
    key
}

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
