//! The Performance view's numbers: what every part in the `Workspace` costs,
//! five ways, read off the DOM and the local asset cache, never the network,
//! never the GPU, so it runs on whatever thread the editor gives it.
//!
//! Roblox publishes no per-instance cost model for edit time (the
//! MicroProfiler only measures a running game), so every figure here is a
//! proxy, named for what it counts rather than for a time it would take:
//!
//! - **Part density**: instances per 32-stud world cell, a part and every
//!   non-part instance beneath it (attachments, decals, scripts, welds).
//! - **Triangles**: what the part draws as. A file mesh's own LOD 0 count, a
//!   union's `TriangleCount` (or its baked mesh), a primitive's unit mesh.
//!   Terrain is left out: it is meshed per chunk, not per instance.
//! - **Texture memory**: decoded RGBA8 bytes (width × height × 4, before any
//!   mip chain) of every image the part samples (its decals and textures,
//!   mesh or `SurfaceAppearance` maps, the maps of the `MaterialVariant` it
//!   draws with, and the clothing a character limb wears), each image split evenly
//!   between the parts sharing it, so the parts' figures add up to every
//!   image counted once.
//! - **Collision**: a relative score, 1 for a primitive or a `Box`
//!   fidelity, 2 for `Hull`, 4 for `Default`, 8 for
//!   `PreciseConvexDecomposition`, a quarter of that for a part that only
//!   touches or is queried, nothing for one that does neither, and double for
//!   a part in an unanchored assembly (joined by welds and motors to no
//!   anchored part), which the physics step simulates every frame.
//! - **Draw cost**: parts sharing a geometry, material, texture set and
//!   opacity can go out as one instanced batch, so a part costs `1 / n` of a
//!   batch where `n` parts share its combination, the sum is the number of
//!   batches.
//!
//! [`Metrics`] keeps one record per part and patches it from the same
//! `Change` log the renderer is patched from; [`Metrics::report`] turns the
//! records into one metric's figures, worst parts and heat bins.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::filemesh::{appearance_of, mesh_part_asset, special_mesh_asset};
use super::material::{service as material_service, Service};
use super::props::{bool_or, cframe_or};
use super::shape::{resolve as resolve_shape, ShapeKind};
use super::{is_drawable, srgb_to_linear, union_triangles, WORKSPACE_CLASS};
use crate::textures::asset_uri;
use crate::textures::clothing::{Limb, Wardrobe};

mod models;

pub use models::{Breakdown, ModelCost};

/// Reads an asset's bytes out of a local cache, `None` when it is not there.
pub type Fetch<'a> = dyn FnMut(&AssetRef) -> Option<Vec<u8>> + 'a;

/// The side of one [`Metric::Density`] cell, in studs.
pub const CELL_STUDS: f32 = 32.0;
/// How many parts [`Report::worst`] lists.
pub const WORST: usize = 20;

/// The heat ramp, coolest first: viridis, sampled at eight even steps,
/// perceptually uniform and readable with every common colour-vision
/// deficiency, which a red-to-green ramp is not.
///
/// Steps rather than a continuous ramp on purpose. Each step is one
/// fill-only `Highlight` on the heat mask (see `renderer::cue`), and a
/// highlight carries one colour, so a smooth ramp would need a per-part
/// colour the mask pass doesn't have. And a step is a range the legend can
/// name ("≥ 201"): a tint read off a gradient is a guess at a position, and
/// eight is about as many swatches as the eye still tells apart on a part.
pub const RAMP: [[u8; 3]; 8] = [
    [0x44, 0x01, 0x54],
    [0x46, 0x32, 0x7e],
    [0x36, 0x5c, 0x8d],
    [0x27, 0x7f, 0x8e],
    [0x1f, 0xa1, 0x87],
    [0x4a, 0xc1, 0x6d],
    [0xa0, 0xda, 0x39],
    [0xfd, 0xe7, 0x25],
];

/// How strongly a heat tint covers the part under it.
const HEAT_ALPHA: f32 = 0.7;

const JOINT_CLASSES: [&str; 8] = [
    "Weld",
    "WeldConstraint",
    "ManualWeld",
    "Motor6D",
    "Motor",
    "Snap",
    "Glue",
    "RigidConstraint",
];

/// The classic clothing a character model's limbs wear, see
/// `textures::clothing`.
const GARMENT_CLASSES: [&str; 3] = ["Shirt", "Pants", "ShirtGraphic"];

/// One way of costing a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    Density,
    Triangles,
    TextureMemory,
    Collision,
    DrawCost,
    /// Every factor of [`ModelCost`] weighed into one score, see `models`.
    Total,
    /// Simulated parts, constraints and costly collision fidelities.
    Physics,
}

impl Metric {
    pub const ALL: [Metric; 5] = [
        Metric::Density,
        Metric::Triangles,
        Metric::TextureMemory,
        Metric::Collision,
        Metric::DrawCost,
    ];

    /// The measures behind the `perf-models` beta switch.
    pub const BETA: [Metric; 2] = [Metric::Total, Metric::Physics];

    pub fn label(self) -> &'static str {
        match self {
            Metric::Density => "Part density",
            Metric::Triangles => "Triangle count",
            Metric::TextureMemory => "Texture memory",
            Metric::Collision => "Collision cost",
            Metric::DrawCost => "Draw cost",
            Metric::Total => "Total (estimate)",
            Metric::Physics => "Physics",
        }
    }

    /// Whether the figures span orders of magnitude, so the ramp is laid
    /// along their logarithm, one 50 000-triangle mesh would otherwise
    /// leave every other part in the coolest bin.
    fn logarithmic(self) -> bool {
        matches!(
            self,
            Metric::Density | Metric::Triangles | Metric::TextureMemory | Metric::Total
        )
    }

    /// `value` with its unit, for a readout.
    pub fn format(self, value: f32) -> String {
        match self {
            Metric::Density => format!("{} instances", grouped(value.round() as u64)),
            Metric::Triangles => format!("{} triangles", grouped(value.round() as u64)),
            Metric::Collision => format!("score {value:.2}"),
            Metric::DrawCost => format!("{value:.3} of a batch"),
            Metric::Total | Metric::Physics => format!("{value:.1} pts"),
            Metric::TextureMemory => self.short(value),
        }
    }

    /// `value` alone, for the legend's narrow labels.
    pub fn short(self, value: f32) -> String {
        match self {
            Metric::Density | Metric::Triangles => grouped(value.round() as u64),
            Metric::TextureMemory => bytes(value as u64),
            Metric::Collision => format!("{value:.2}"),
            Metric::DrawCost => format!("{value:.3}"),
            Metric::Total | Metric::Physics => format!("{value:.1}"),
        }
    }
}

/// One colour of the heat ramp and every part painted in it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatBin {
    /// Linear RGB, what the renderer blends in.
    pub color: [f32; 3],
    pub alpha: f32,
    pub parts: Vec<Ref>,
}

/// What hovering a part reads out.
#[derive(Debug, Clone, PartialEq)]
pub struct Readout {
    pub name: String,
    pub value: f32,
    pub detail: String,
}

/// One metric's figures over the whole place.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub metric: Metric,
    pub summary: String,
    /// One entry per [`RAMP`] colour: the sRGB swatch and the smallest value
    /// it stands for.
    pub legend: Vec<([u8; 3], String)>,
    pub bins: Vec<HeatBin>,
    /// The [`WORST`] costliest parts, costliest first; a part costing nothing
    /// is never listed.
    pub worst: Vec<Ref>,
    /// The [`WORST`] costliest models, see `models`.
    pub models: Vec<ModelCost>,
    readouts: HashMap<Ref, Readout>,
}

impl Report {
    pub fn readout(&self, referent: Ref) -> Option<&Readout> {
        self.readouts.get(&referent)
    }

    pub fn parts(&self) -> usize {
        self.readouts.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Geometry {
    Shape(ShapeKind),
    Mesh(AssetRef),
    Union(Option<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Fidelity {
    Default,
    Hull,
    Box,
    Precise,
}

impl Fidelity {
    /// `Enum.CollisionFidelity`: Default=0, Hull=1, Box=2,
    /// PreciseConvexDecomposition=3.
    fn of(properties: &BTreeMap<String, Variant>) -> Fidelity {
        match properties.get("CollisionFidelity") {
            Some(Variant::Enum(1)) => Fidelity::Hull,
            Some(Variant::Enum(2)) => Fidelity::Box,
            Some(Variant::Enum(3)) => Fidelity::Precise,
            _ => Fidelity::Default,
        }
    }

    fn weight(self) -> f32 {
        match self {
            Fidelity::Box => 1.0,
            Fidelity::Hull => 2.0,
            Fidelity::Default => 4.0,
            Fidelity::Precise => 8.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Fidelity::Default => "Default fidelity",
            Fidelity::Hull => "Hull fidelity",
            Fidelity::Box => "Box fidelity",
            Fidelity::Precise => "PreciseConvexDecomposition",
        }
    }
}

/// What one part costs, as read off the DOM. Asset-backed figures are looked
/// up at report time, so an asset landing in the cache later needs no
/// re-read of the part.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    cell: [i32; 3],
    /// The part and every non-part instance beneath it.
    instances: u32,
    geometry: Geometry,
    textures: Vec<AssetRef>,
    /// `None` for a primitive, which collides as its own shape.
    fidelity: Option<Fidelity>,
    collides: bool,
    queried: bool,
    anchored: bool,
    batch: u64,
    /// The outermost `Model` the part sits in, below the `Workspace`.
    model: Option<Ref>,
    extra: models::Extra,
}

/// Every part's record, kept in step with the DOM.
pub struct Metrics {
    database: &'static ReflectionDatabase,
    entries: HashMap<Ref, Entry>,
    /// Every weld or motor in the `Workspace`, as the two parts it joins.
    joints: HashMap<Ref, (Ref, Ref)>,
    /// Triangle counts by mesh, `None` while the mesh is not in the cache.
    meshes: HashMap<AssetRef, Option<u32>>,
    /// Decoded bytes by image, `None` while the image is not in the cache.
    images: HashMap<AssetRef, Option<u64>>,
    shapes: Vec<(ShapeKind, u32)>,
    /// Union triangle counts by bake, see `union::triangles`.
    unions: HashMap<AssetRef, Option<u32>>,
    /// `MaterialService`, for the variant maps a part samples.
    materials: Service,
    /// Every garment in the `Workspace`, by the model it dresses.
    garments: HashMap<Ref, Ref>,
    /// Every physics constraint in the `Workspace`, as the parts its
    /// attachments sit on.
    constraints: HashMap<Ref, Vec<Ref>>,
    /// Every `Model` in the `Workspace`, by name.
    models: HashMap<Ref, String>,
}

impl Metrics {
    pub fn new(dom: &WeakDom, fetch: &mut Fetch<'_>) -> Self {
        let database = ReflectionDatabase::shared();
        let mut metrics = Metrics {
            database,
            entries: HashMap::new(),
            joints: HashMap::new(),
            meshes: HashMap::new(),
            images: HashMap::new(),
            shapes: Vec::new(),
            unions: HashMap::new(),
            materials: material_service(dom, database),
            garments: HashMap::new(),
            constraints: HashMap::new(),
            models: HashMap::new(),
        };
        let all: Vec<Ref> = super::workspace_descendants(dom, metrics.database).collect();
        for referent in all {
            metrics.read(dom, referent, true, fetch);
        }
        metrics
    }

    /// Brings the records in line with one edit's `Change` log, `dom` being
    /// the tree as it stands after it. Only what the log names is re-read:
    /// the instance itself, the part it hangs under (whose instance count,
    /// mesh or textures it may be), and, for an instance that arrived,
    /// moved or changed class, everything beneath it. A garment's edit
    /// re-reads the limbs of the model it dresses, and an edit to
    /// `MaterialService` or a variant in it every part, since any of them
    /// may draw with it.
    pub fn patch(&mut self, dom: &WeakDom, changes: &[Change], fetch: &mut Fetch<'_>) {
        let mut shallow = HashSet::new();
        let mut deep = HashSet::new();
        let mut materials = false;
        for change in changes {
            let (Change::Property { referent, .. }
            | Change::Parent { referent, .. }
            | Change::Added(referent)
            | Change::Removed(referent)
            | Change::Class(referent)) = *change;
            let class = dom.get(referent).map(|i| i.class());
            materials |= self.materials.referents.contains(&referent)
                || matches!(class, Some("MaterialService" | "MaterialVariant"));
            if let Some(model) = self.garments.remove(&referent) {
                shallow.extend(dom.get(model).into_iter().flat_map(|m| m.children()));
            }
            if class.is_some_and(|class| GARMENT_CLASSES.contains(&class)) {
                let model = dom.parent(referent);
                shallow.extend(
                    model
                        .and_then(|m| dom.get(m))
                        .into_iter()
                        .flat_map(|m| m.children()),
                );
            }
            match change {
                Change::Property { referent, .. } => {
                    shallow.insert(*referent);
                }
                Change::Added(referent) | Change::Class(referent) => {
                    deep.insert(*referent);
                }
                Change::Parent { referent, old, .. } => {
                    deep.insert(*referent);
                    shallow.extend(*old);
                }
                Change::Removed(referent) => {
                    self.entries.remove(referent);
                    self.joints.remove(referent);
                    self.constraints.remove(referent);
                    self.models.remove(referent);
                }
            }
        }
        // A removal names the instance, not the part it hung under.
        self.entries
            .retain(|referent, _| dom.get(*referent).is_some());
        self.joints
            .retain(|referent, _| dom.get(*referent).is_some());
        self.constraints
            .retain(|referent, _| dom.get(*referent).is_some());
        self.models
            .retain(|referent, _| dom.get(*referent).is_some());

        let mut owners = HashSet::new();
        for &referent in shallow.iter().chain(&deep) {
            owners.extend(self.owner(dom, referent));
        }
        for referent in deep {
            let subtree: Vec<Ref> = super::descendants_of(dom, referent).collect();
            for child in subtree {
                self.read(dom, child, false, fetch);
            }
        }
        for referent in shallow.into_iter().chain(owners) {
            self.read(dom, referent, false, fetch);
        }
        if materials {
            self.materials = material_service(dom, self.database);
            let parts: Vec<Ref> = self.entries.keys().copied().collect();
            for part in parts {
                self.read(dom, part, true, fetch);
            }
        }
    }

    /// Looks again for every asset the cache did not have; `true` when one
    /// has landed since, so the figures are worth reporting again.
    pub fn retry(&mut self, fetch: &mut Fetch<'_>) -> bool {
        let mut landed = false;
        for (asset, count) in self.meshes.iter_mut().filter(|(_, c)| c.is_none()) {
            *count = mesh_triangles(fetch, asset);
            landed |= count.is_some();
        }
        for (asset, size) in self.images.iter_mut().filter(|(_, s)| s.is_none()) {
            *size = image_bytes(fetch, asset);
            landed |= size.is_some();
        }
        landed
    }

    /// How many parts are on record.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn report(&self, metric: Metric) -> Report {
        let (values, summary, details) = match metric {
            Metric::Density => self.density(),
            Metric::Triangles => self.triangles(),
            Metric::TextureMemory => self.texture_memory(),
            Metric::Collision => self.collision(),
            Metric::DrawCost => self.draw_cost(),
            Metric::Total => self.total(),
            Metric::Physics => self.physics(),
        };
        let max = values.values().copied().fold(0.0_f32, f32::max);
        let scale = |value: f32| -> f32 {
            if max <= 0.0 {
                0.0
            } else if metric.logarithmic() {
                value.ln_1p() / max.ln_1p()
            } else {
                value / max
            }
        };
        let unscale = |t: f32| -> f32 {
            if metric.logarithmic() {
                (t * max.ln_1p()).exp_m1()
            } else {
                t * max
            }
        };

        let steps = RAMP.len();
        let mut bins: Vec<HeatBin> = RAMP
            .iter()
            .map(|color| HeatBin {
                color: color.map(|channel| srgb_to_linear(f32::from(channel) / 255.0)),
                alpha: HEAT_ALPHA,
                parts: Vec::new(),
            })
            .collect();
        for (&referent, &value) in &values {
            let step = ((scale(value) * steps as f32) as usize).min(steps - 1);
            bins[step].parts.push(referent);
        }
        for bin in &mut bins {
            bin.parts.sort_unstable();
        }
        bins.retain(|bin| !bin.parts.is_empty());

        let legend = RAMP
            .iter()
            .enumerate()
            .map(|(step, &color)| (color, metric.short(unscale(step as f32 / steps as f32))))
            .collect();

        let mut ranked: Vec<(Ref, f32)> = values
            .iter()
            .filter(|(_, &value)| value > 0.0)
            .map(|(&referent, &value)| (referent, value))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let worst = ranked.iter().take(WORST).map(|&(r, _)| r).collect();

        let mut details = details;
        let readouts = values
            .into_iter()
            .map(|(referent, value)| {
                let readout = Readout {
                    name: self.entries[&referent].name.clone(),
                    value,
                    detail: details.remove(&referent).unwrap_or_default(),
                };
                (referent, readout)
            })
            .collect();

        Report {
            metric,
            summary,
            legend,
            bins,
            worst,
            models: self.models_ranked(metric),
            readouts,
        }
    }

    fn density(&self) -> Figures {
        let mut cells: HashMap<[i32; 3], u32> = HashMap::new();
        for entry in self.entries.values() {
            *cells.entry(entry.cell).or_default() += entry.instances;
        }
        let busiest = cells.values().copied().max().unwrap_or(0);
        let mut details = HashMap::new();
        let values = self
            .entries
            .iter()
            .map(|(&referent, entry)| {
                let count = cells[&entry.cell];
                details.insert(
                    referent,
                    format!("in its {CELL_STUDS}-stud cell, {} its own", entry.instances),
                );
                (referent, count as f32)
            })
            .collect();
        let summary = format!(
            "{} cells hold parts; the busiest holds {} instances",
            grouped(cells.len() as u64),
            grouped(u64::from(busiest))
        );
        (values, summary, details)
    }

    fn triangles(&self) -> Figures {
        let mut total = 0_u64;
        let mut unknown = 0;
        let mut details = HashMap::new();
        let mut values = HashMap::new();
        for (&referent, entry) in &self.entries {
            let (count, detail) = match &entry.geometry {
                Geometry::Shape(kind) => {
                    let count = self.shape_triangles(*kind);
                    (Some(count), format!("{kind:?} primitive"))
                }
                Geometry::Mesh(asset) => match self.meshes.get(asset).copied().flatten() {
                    Some(count) => (Some(count), "mesh".to_string()),
                    None => (None, "mesh not downloaded yet".to_string()),
                },
                Geometry::Union(Some(count)) => (Some(*count), "union".to_string()),
                Geometry::Union(None) => (None, "union with no baked mesh to count".to_string()),
            };
            if count.is_none() {
                unknown += 1;
            }
            let count = count.unwrap_or(0);
            total += u64::from(count);
            values.insert(referent, count as f32);
            details.insert(referent, detail);
        }
        let mut summary = format!("{} triangles, terrain excluded", grouped(total));
        if unknown > 0 {
            summary.push_str(&format!("; {unknown} parts not counted yet"));
        }
        (values, summary, details)
    }

    fn texture_memory(&self) -> Figures {
        let mut users: HashMap<&AssetRef, u32> = HashMap::new();
        for entry in self.entries.values() {
            for texture in &entry.textures {
                *users.entry(texture).or_default() += 1;
            }
        }
        let size = |asset: &AssetRef| self.images.get(asset).copied().flatten();
        let total: u64 = users.keys().filter_map(|asset| size(asset)).sum();
        let missing = users.keys().filter(|asset| size(asset).is_none()).count();

        let mut details = HashMap::new();
        let values = self
            .entries
            .iter()
            .map(|(&referent, entry)| {
                let share: f64 = entry
                    .textures
                    .iter()
                    .filter_map(|asset| Some(size(asset)? as f64 / f64::from(users[asset])))
                    .sum();
                let shared = entry.textures.iter().filter(|a| users[a] > 1).count();
                details.insert(
                    referent,
                    match entry.textures.len() {
                        0 => "no textures".to_string(),
                        n => format!(
                            "{n} texture{}{}",
                            if n == 1 { "" } else { "s" },
                            if shared > 0 {
                                format!(", {shared} shared")
                            } else {
                                String::new()
                            }
                        ),
                    },
                );
                (referent, share as f32)
            })
            .collect();
        let mut summary = format!(
            "{} decoded across {} textures, each counted once",
            bytes(total),
            grouped((users.len() - missing) as u64)
        );
        if missing > 0 {
            summary.push_str(&format!("; {missing} not downloaded yet"));
        }
        (values, summary, details)
    }

    fn collision(&self) -> Figures {
        let assemblies = self.assemblies();
        let mut details = HashMap::new();
        let mut total = 0.0;
        let values = self
            .entries
            .iter()
            .map(|(&referent, entry)| {
                let mut notes = Vec::new();
                let mut score = match entry.fidelity {
                    Some(fidelity) => {
                        notes.push(fidelity.label().to_string());
                        fidelity.weight()
                    }
                    None => {
                        notes.push("primitive".to_string());
                        1.0
                    }
                };
                if !entry.collides {
                    if entry.queried {
                        score *= 0.25;
                        notes.push("CanCollide off".to_string());
                    } else {
                        score = 0.0;
                        notes.push("no collision, touch or query".to_string());
                    }
                }
                if let Some(&size) = assemblies.get(&referent) {
                    score *= 2.0;
                    notes.push(match size {
                        1 => "unanchored".to_string(),
                        n => format!("unanchored assembly of {n}"),
                    });
                }
                total += score;
                details.insert(referent, notes.join(", "));
                (referent, score)
            })
            .collect();
        let simulated = assemblies.len();
        let summary = format!(
            "total score {total:.0}; {} parts simulated (unanchored)",
            grouped(simulated as u64)
        );
        (values, summary, details)
    }

    /// Every part in an unanchored assembly, with the assembly's size: parts
    /// joined through welds and motors, none of them anchored.
    fn assemblies(&self) -> HashMap<Ref, usize> {
        let mut root: HashMap<Ref, Ref> = HashMap::new();
        fn find(root: &mut HashMap<Ref, Ref>, at: Ref) -> Ref {
            let mut top = at;
            while let Some(&up) = root.get(&top).filter(|&&up| up != top) {
                top = up;
            }
            root.insert(at, top);
            top
        }
        for &(a, b) in self.joints.values() {
            if self.entries.contains_key(&a) && self.entries.contains_key(&b) {
                let (ra, rb) = (find(&mut root, a), find(&mut root, b));
                if ra != rb {
                    root.insert(ra, rb);
                }
            }
        }
        let mut anchored: HashSet<Ref> = HashSet::new();
        let mut sizes: HashMap<Ref, usize> = HashMap::new();
        let mut groups = Vec::with_capacity(self.entries.len());
        for (&referent, entry) in &self.entries {
            let group = find(&mut root, referent);
            if entry.anchored {
                anchored.insert(group);
            }
            *sizes.entry(group).or_default() += 1;
            groups.push((referent, group));
        }
        groups
            .into_iter()
            .filter(|(_, group)| !anchored.contains(group))
            .map(|(referent, group)| (referent, sizes[&group]))
            .collect()
    }

    fn draw_cost(&self) -> Figures {
        let mut sharing: HashMap<u64, u32> = HashMap::new();
        for entry in self.entries.values() {
            *sharing.entry(entry.batch).or_default() += 1;
        }
        let mut details = HashMap::new();
        let values = self
            .entries
            .iter()
            .map(|(&referent, entry)| {
                let n = sharing[&entry.batch];
                details.insert(
                    referent,
                    match n {
                        1 => "a batch of its own".to_string(),
                        n => format!(
                            "shares one batch with {} other parts",
                            grouped(u64::from(n - 1))
                        ),
                    },
                );
                (referent, 1.0 / n as f32)
            })
            .collect();
        let summary = format!(
            "about {} batches for {} parts",
            grouped(sharing.len() as u64),
            grouped(self.entries.len() as u64)
        );
        (values, summary, details)
    }

    fn shape_triangles(&self, kind: ShapeKind) -> u32 {
        self.shapes
            .iter()
            .find(|(known, _)| *known == kind)
            .map_or(12, |&(_, count)| count)
    }

    /// The part `referent` belongs to: itself if it is one, else its nearest
    /// part ancestor.
    fn owner(&self, dom: &WeakDom, referent: Ref) -> Option<Ref> {
        let mut at = Some(referent);
        while let Some(current) = at {
            if is_drawable(dom, self.database, current) {
                return Some(current);
            }
            at = dom.parent(current);
        }
        None
    }

    fn in_workspace(&self, dom: &WeakDom, referent: Ref) -> bool {
        let mut at = Some(referent);
        while let Some(current) = at {
            if dom
                .get(current)
                .is_some_and(|i| self.database.is_subclass_of(i.class(), WORKSPACE_CLASS))
            {
                return true;
            }
            at = dom.parent(current);
        }
        false
    }

    /// Re-reads one instance's record, dropping it if it no longer qualifies.
    /// `inside` skips the `Workspace` check for a caller already walking it.
    fn read(&mut self, dom: &WeakDom, referent: Ref, inside: bool, fetch: &mut Fetch<'_>) {
        let Some(instance) = dom.get(referent) else {
            self.entries.remove(&referent);
            self.joints.remove(&referent);
            self.constraints.remove(&referent);
            self.models.remove(&referent);
            return;
        };
        let inside = inside || self.in_workspace(dom, referent);
        let joint = JOINT_CLASSES.contains(&instance.class());
        if joint {
            let part = |key: &str| match instance.properties().get(key) {
                Some(&Variant::Ref(r)) if r.value() != 0 => Some(r),
                _ => None,
            };
            match (inside, part("Part0"), part("Part1")) {
                (true, Some(a), Some(b)) => self.joints.insert(referent, (a, b)),
                _ => self.joints.remove(&referent),
            };
        }
        if GARMENT_CLASSES.contains(&instance.class()) {
            match (inside, dom.parent(referent)) {
                (true, Some(model)) => self.garments.insert(referent, model),
                _ => self.garments.remove(&referent),
            };
        }
        self.read_extra(dom, referent, inside);
        if inside && is_drawable(dom, self.database, referent) {
            let entry = self.entry(dom, referent, fetch);
            self.entries.insert(referent, entry);
        } else {
            self.entries.remove(&referent);
        }
    }

    fn entry(&mut self, dom: &WeakDom, referent: Ref, fetch: &mut Fetch<'_>) -> Entry {
        let database = self.database;
        let instance = dom.get(referent).expect("read checked it exists");
        let properties = instance.properties();
        let size = match properties.get("size") {
            Some(&Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
            _ => Vec3::ONE,
        };
        let position = cframe_or(properties, "CFrame").w_axis.truncate();
        let cell = (position / CELL_STUDS).floor().as_ivec3().to_array();

        let mut instances = 1;
        let mut extra = models::Extra::of(properties, size);
        let mut textures = Vec::new();
        let mut pending: Vec<Ref> = instance.children().to_vec();
        while let Some(child) = pending.pop() {
            let Some(child) = dom.get(child) else {
                continue;
            };
            if is_drawable(dom, database, child.referent()) {
                continue;
            }
            instances += 1;
            extra.count_light(child);
            pending.extend_from_slice(child.children());
            if matches!(child.class(), "Decal" | "Texture") {
                textures.extend(child.properties().get("Texture").and_then(reference));
            }
        }

        let union = database.is_subclass_of(instance.class(), "PartOperation");
        let geometry = if let Some((mesh, _)) = mesh_part_asset(database, instance) {
            match appearance_of(dom, instance) {
                Some(appearance) => textures.extend(appearance.maps.iter().flatten().cloned()),
                None => textures.extend(properties.get("TextureID").and_then(reference)),
            }
            Geometry::Mesh(mesh)
        } else if let Some((child, mesh, _)) = special_mesh_asset(dom, instance) {
            textures.extend(child.properties().get("TextureId").and_then(reference));
            Geometry::Mesh(mesh)
        } else if union {
            Geometry::Union(union_triangles(properties, fetch, &mut self.unions))
        } else {
            Geometry::Shape(resolve_shape(dom, database, instance, size).kind)
        };
        textures.extend(self.materials.variant_maps(properties, database));
        let limb = Limb::r6(instance.name()).or_else(|| Limb::r15(instance.name()));
        if let Some(wardrobe) = limb
            .zip(dom.parent(referent))
            .and_then(|(limb, model)| Wardrobe::of(dom, model).map(|wardrobe| (limb, wardrobe)))
        {
            let (limb, wardrobe) = wardrobe;
            textures.extend(
                wardrobe
                    .for_limb(limb)
                    .into_iter()
                    .map(|(_, layer)| layer.image.clone()),
            );
        }
        textures.sort_by_key(|asset| format!("{asset:?}"));
        textures.dedup();

        match &geometry {
            Geometry::Mesh(mesh) if !self.meshes.contains_key(mesh) => {
                let count = mesh_triangles(fetch, mesh);
                self.meshes.insert(mesh.clone(), count);
            }
            Geometry::Shape(kind) if !self.shapes.iter().any(|(k, _)| k == kind) => {
                self.shapes.push((*kind, unit_triangles(*kind)));
            }
            _ => {}
        }
        for texture in &textures {
            if !self.images.contains_key(texture) {
                let size = image_bytes(fetch, texture);
                self.images.insert(texture.clone(), size);
            }
        }

        let collides = bool_or(properties, "CanCollide", true);
        let queried =
            bool_or(properties, "CanTouch", true) || bool_or(properties, "CanQuery", true);
        let fidelity = (union || database.is_subclass_of(instance.class(), "MeshPart"))
            .then(|| Fidelity::of(properties));
        let material = match properties.get("Material") {
            Some(&Variant::Enum(raw)) => raw,
            _ => 256,
        };
        let translucent =
            matches!(properties.get("Transparency"), Some(&Variant::Float32(t)) if t > 0.0);

        let mut hasher = DefaultHasher::new();
        match &geometry {
            Geometry::Shape(kind) => format!("{kind:?}").hash(&mut hasher),
            Geometry::Mesh(mesh) => mesh.hash(&mut hasher),
            // Every union carves a mesh of its own.
            Geometry::Union(_) => referent.hash(&mut hasher),
        }
        (material, translucent, &textures).hash(&mut hasher);

        Entry {
            name: instance.name().to_string(),
            cell,
            instances,
            geometry,
            textures,
            fidelity,
            collides,
            queried,
            anchored: bool_or(properties, "Anchored", false),
            batch: hasher.finish(),
            model: self.outer_model(dom, referent),
            extra,
        }
    }
}

/// Values, the summary line, and each part's detail line.
type Figures = (HashMap<Ref, f32>, String, HashMap<Ref, String>);

fn reference(value: &Variant) -> Option<AssetRef> {
    match AssetRef::parse(asset_uri(value)?).ok()? {
        AssetRef::Empty => None,
        asset => Some(asset),
    }
}

fn mesh_triangles(fetch: &mut Fetch<'_>, mesh: &AssetRef) -> Option<u32> {
    let mesh = rbx_mesh::parse(&fetch(mesh)?).ok()?;
    u32::try_from(mesh.triangle_count()).ok()
}

fn image_bytes(fetch: &mut Fetch<'_>, image: &AssetRef) -> Option<u64> {
    let (width, height) = rbx_assets::image_dimensions(&fetch(image)?)?;
    Some(u64::from(width) * u64::from(height) * 4)
}

/// The triangles of the unit mesh the renderer instances for `kind`.
fn unit_triangles(kind: ShapeKind) -> u32 {
    use crate::shapes;
    let data = match kind {
        ShapeKind::Box => return 12,
        ShapeKind::Ball => shapes::sphere(),
        ShapeKind::CylinderX => shapes::cylinder_x(),
        ShapeKind::CylinderY => shapes::cylinder_y(),
        ShapeKind::Wedge => shapes::wedge(),
        ShapeKind::CornerWedge => shapes::corner_wedge(),
        ShapeKind::Truss {
            axis,
            segments,
            style,
        } => shapes::oriented(shapes::truss(segments, style), axis),
    };
    (data.indices.len() / 3) as u32
}

/// `1234567` as `1,234,567`.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn bytes(value: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = value as f64;
    if value < KIB {
        format!("{value} B")
    } else if value < KIB * KIB {
        format!("{:.1} KB", value / KIB)
    } else {
        format!("{:.1} MB", value / (KIB * KIB))
    }
}

#[cfg(test)]
#[path = "perf/tests.rs"]
mod tests;
