//! The `perf-models` beta's figures: every factor a part costs weighed into
//! one score, and the place's models ranked by the sum of their parts'.
//!
//! The weights are an estimate for comparing the parts of one place with
//! each other. Roblox publishes no such costs, and these don't claim to be
//! its own: a point is about a thousand triangles' worth of work, and the
//! other factors are scaled to sit beside that.
//!
//! - **Triangles**: 1 point per 1 000.
//! - **Texture memory**: 1 point per MiB decoded, shared images split as
//!   [`Metric::TextureMemory`] splits them.
//! - **Instances**: 1 point per 50, the part and every non-part beneath it.
//! - **Physics**: 1 per part in an unanchored assembly, 0.5 per physics
//!   constraint (split between the parts its attachments sit on), 2 for a
//!   `PreciseConvexDecomposition` collision and 1 for `Default` fidelity on a
//!   mesh or union over [`COMPLEX_MESH`] triangles. A part that neither
//!   collides nor is touched or queried pays no fidelity.
//! - **Shadows**: a mesh or union over [`DENSE_MESH`] triangles that casts a
//!   shadow draws them again into the shadow map, half a point per 1 000;
//!   each enabled light with `Shadows` on beneath a part is 3.
//! - **Overdraw**: a part drawn blended (translucent, or `Glass`) shades
//!   whatever is behind it again, 1 point per 500 square studs of surface.
//!
//! A part's model is the outermost `Model` holding it below the `Workspace`;
//! a part in no model is a row of its own.

use std::collections::{BTreeMap, HashMap};

use glam::Vec3;
use rbx_dom::{Instance, Ref, Variant, WeakDom};

use super::{bytes, grouped, Fidelity, Figures, Geometry, Metric, Metrics, WORST};
use crate::scene::props::bool_or;
use crate::scene::WORKSPACE_CLASS;

/// `Default` fidelity on a mesh over this many triangles decomposes into
/// many hulls.
pub const COMPLEX_MESH: f32 = 1_000.0;
/// A shadow-casting mesh over this many triangles is costed again for its
/// shadow.
pub const DENSE_MESH: f32 = 5_000.0;

const PER_TRIANGLE: f32 = 1.0 / 1_000.0;
const PER_BYTE: f32 = 1.0 / (1024.0 * 1024.0);
const PER_INSTANCE: f32 = 1.0 / 50.0;
const PER_SIMULATED: f32 = 1.0;
const PER_CONSTRAINT: f32 = 0.5;
const PRECISE: f32 = 2.0;
const COMPLEX_DEFAULT: f32 = 1.0;
const PER_SHADOW_TRIANGLE: f32 = 0.5 / 1_000.0;
const PER_SHADOW_LIGHT: f32 = 3.0;
const PER_BLENDED_STUD: f32 = 1.0 / 500.0;

/// `Enum.Material.Glass`.
const GLASS: u32 = 1568;
const LIGHT_CLASSES: [&str; 3] = ["PointLight", "SpotLight", "SurfaceLight"];

/// What a part's own record adds for this module: read with the rest of
/// the part in `Metrics::entry`.
#[derive(Debug, Clone, Default)]
pub(super) struct Extra {
    cast_shadow: bool,
    /// Surface area in square studs when the part is drawn blended, else 0.
    blended: f32,
    shadow_lights: u32,
}

impl Extra {
    pub(super) fn of(properties: &BTreeMap<String, Variant>, size: Vec3) -> Extra {
        let transparency = match properties.get("Transparency") {
            Some(&Variant::Float32(t)) => t,
            _ => 0.0,
        };
        let glass = matches!(properties.get("Material"), Some(&Variant::Enum(GLASS)));
        let blended = (transparency > 0.0 || glass) && transparency < 1.0;
        Extra {
            cast_shadow: bool_or(properties, "CastShadow", true),
            blended: if blended {
                2.0 * (size.x * size.y + size.y * size.z + size.z * size.x)
            } else {
                0.0
            },
            shadow_lights: 0,
        }
    }

    /// Counts `instance`, one of the non-part instances beneath the part,
    /// if it is a light casting shadows.
    pub(super) fn count_light(&mut self, instance: &Instance) {
        let properties = instance.properties();
        if LIGHT_CLASSES.contains(&instance.class())
            && bool_or(properties, "Shadows", false)
            && bool_or(properties, "Enabled", true)
        {
            self.shadow_lights += 1;
        }
    }
}

/// What a part, or a model's parts together, cost, factor by factor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Breakdown {
    pub triangles: f32,
    pub texture_bytes: f32,
    pub instances: f32,
    pub parts: u32,
    /// Parts in an unanchored assembly.
    pub simulated: u32,
    /// Physics constraints, each split between the parts it joins.
    pub constraints: f32,
    /// Points for costly collision fidelities.
    pub fidelity: f32,
    /// Triangles drawn again into the shadow map.
    pub shadow_triangles: f32,
    pub shadow_lights: u32,
    /// Square studs of blended surface.
    pub blended: f32,
}

impl Breakdown {
    /// Each factor's points, in the module doc's order.
    pub fn factors(&self) -> [(&'static str, f32); 6] {
        [
            ("Triangles", self.triangles * PER_TRIANGLE),
            ("Texture memory", self.texture_bytes * PER_BYTE),
            ("Instances", self.instances * PER_INSTANCE),
            ("Physics", self.physics()),
            (
                "Shadows",
                self.shadow_triangles * PER_SHADOW_TRIANGLE
                    + self.shadow_lights as f32 * PER_SHADOW_LIGHT,
            ),
            ("Overdraw", self.blended * PER_BLENDED_STUD),
        ]
    }

    pub fn physics(&self) -> f32 {
        self.simulated as f32 * PER_SIMULATED + self.constraints * PER_CONSTRAINT + self.fidelity
    }

    pub fn total(&self) -> f32 {
        self.factors().iter().map(|(_, points)| points).sum()
    }

    /// The raw figures behind the points, zeros left out.
    pub fn figures(&self) -> String {
        let mut out = vec![
            format!("{} triangles", grouped(self.triangles as u64)),
            format!("{} textures", bytes(self.texture_bytes as u64)),
            format!(
                "{} instances in {} part{}",
                grouped(self.instances as u64),
                grouped(u64::from(self.parts)),
                if self.parts == 1 { "" } else { "s" }
            ),
        ];
        let counted = [
            (self.simulated as f32, "simulated"),
            (self.constraints, "constraints"),
            (self.shadow_lights as f32, "shadowed lights"),
            (self.blended, "studs² blended"),
        ];
        out.extend(
            counted
                .into_iter()
                .filter(|(value, _)| *value > 0.0)
                .map(|(value, label)| format!("{} {label}", grouped(value.round() as u64))),
        );
        out.join(", ")
    }

    /// The costliest factors, as `Triangles 4.1 · Physics 2.0`.
    pub fn leading(&self) -> String {
        let mut factors: Vec<_> = self
            .factors()
            .into_iter()
            .filter(|(_, points)| *points >= 0.05)
            .collect();
        factors.sort_by(|a, b| b.1.total_cmp(&a.1));
        factors
            .iter()
            .take(3)
            .map(|(label, points)| format!("{label} {points:.1}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn add(&mut self, other: &Breakdown) {
        self.triangles += other.triangles;
        self.texture_bytes += other.texture_bytes;
        self.instances += other.instances;
        self.parts += other.parts;
        self.simulated += other.simulated;
        self.constraints += other.constraints;
        self.fidelity += other.fidelity;
        self.shadow_triangles += other.shadow_triangles;
        self.shadow_lights += other.shadow_lights;
        self.blended += other.blended;
    }
}

/// One row of the model ranking.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCost {
    /// The model, or the part itself for one in no model.
    pub referent: Ref,
    pub name: String,
    pub cost: Breakdown,
}

impl Metrics {
    fn is_model(&self, class: &str) -> bool {
        self.database.is_subclass_of(class, "Model")
            && !self.database.is_subclass_of(class, WORKSPACE_CLASS)
    }

    /// Keeps the model names and constraints in step with `referent`.
    pub(super) fn read_extra(&mut self, dom: &WeakDom, referent: Ref, inside: bool) {
        let Some(instance) = dom.get(referent) else {
            return;
        };
        if inside && self.is_model(instance.class()) {
            self.models.insert(referent, instance.name().to_string());
        } else {
            self.models.remove(&referent);
        }
        if inside && self.database.is_subclass_of(instance.class(), "Constraint") {
            let parts = ["Attachment0", "Attachment1"]
                .into_iter()
                .filter_map(|key| match instance.properties().get(key) {
                    Some(&Variant::Ref(r)) if r.value() != 0 => self.owner(dom, r),
                    _ => None,
                })
                .collect();
            self.constraints.insert(referent, parts);
        } else {
            self.constraints.remove(&referent);
        }
    }

    pub(super) fn outer_model(&self, dom: &WeakDom, referent: Ref) -> Option<Ref> {
        let mut found = None;
        let mut at = dom.parent(referent);
        while let Some(current) = at {
            let instance = dom.get(current)?;
            if self
                .database
                .is_subclass_of(instance.class(), WORKSPACE_CLASS)
            {
                break;
            }
            if self.is_model(instance.class()) {
                found = Some(current);
            }
            at = dom.parent(current);
        }
        found
    }

    /// Every part's [`Breakdown`].
    pub(super) fn breakdowns(&self) -> HashMap<Ref, Breakdown> {
        let triangles = self.triangles().0;
        let textures = self.texture_memory().0;
        let simulated = self.assemblies();
        let mut constraints: HashMap<Ref, f32> = HashMap::new();
        for parts in self.constraints.values() {
            let present: Vec<&Ref> = parts
                .iter()
                .filter(|part| self.entries.contains_key(part))
                .collect();
            for part in &present {
                *constraints.entry(**part).or_default() += 1.0 / present.len() as f32;
            }
        }
        self.entries
            .iter()
            .map(|(&referent, entry)| {
                let triangles = triangles[&referent];
                let meshy = matches!(entry.geometry, Geometry::Mesh(_) | Geometry::Union(_));
                let interacts = entry.collides || entry.queried;
                let fidelity = match entry.fidelity {
                    Some(Fidelity::Precise) if interacts => PRECISE,
                    Some(Fidelity::Default) if interacts && meshy && triangles > COMPLEX_MESH => {
                        COMPLEX_DEFAULT
                    }
                    _ => 0.0,
                };
                let dense = entry.extra.cast_shadow && meshy && triangles > DENSE_MESH;
                let cost = Breakdown {
                    triangles,
                    texture_bytes: textures[&referent],
                    instances: entry.instances as f32,
                    parts: 1,
                    simulated: u32::from(simulated.contains_key(&referent)),
                    constraints: constraints.get(&referent).copied().unwrap_or(0.0),
                    fidelity,
                    shadow_triangles: if dense { triangles } else { 0.0 },
                    shadow_lights: entry.extra.shadow_lights,
                    blended: entry.extra.blended,
                };
                (referent, cost)
            })
            .collect()
    }

    pub(super) fn total(&self) -> Figures {
        let costs = self.breakdowns();
        let total: f32 = costs.values().map(Breakdown::total).sum();
        let details = costs
            .iter()
            .map(|(&referent, cost)| (referent, cost.leading()))
            .collect();
        let values = costs
            .iter()
            .map(|(&referent, cost)| (referent, cost.total()))
            .collect();
        let summary = format!("{total:.0} points in all, an estimate, not Roblox's own costs");
        (values, summary, details)
    }

    pub(super) fn physics(&self) -> Figures {
        let costs = self.breakdowns();
        let mut sum = Breakdown::default();
        let mut details = HashMap::new();
        let mut values = HashMap::new();
        for (&referent, cost) in &costs {
            sum.add(cost);
            let mut notes = Vec::new();
            if cost.simulated > 0 {
                notes.push("simulated".to_string());
            }
            if cost.constraints > 0.0 {
                notes.push(format!("{:.1} constraints", cost.constraints));
            }
            if cost.fidelity > 0.0 {
                let fidelity = self.entries[&referent].fidelity.map_or("", Fidelity::label);
                notes.push(fidelity.to_string());
            }
            details.insert(referent, notes.join(", "));
            values.insert(referent, cost.physics());
        }
        let summary = format!(
            "{} parts simulated, {} constraints, {:.0} points of collision fidelity",
            grouped(u64::from(sum.simulated)),
            grouped(sum.constraints.round() as u64),
            sum.fidelity
        );
        (values, summary, details)
    }

    /// The [`WORST`] costliest models by `metric`'s score, costliest first;
    /// empty for a metric outside [`Metric::BETA`].
    pub(super) fn models_ranked(&self, metric: Metric) -> Vec<ModelCost> {
        let score = match metric {
            Metric::Total => Breakdown::total,
            Metric::Physics => Breakdown::physics,
            _ => return Vec::new(),
        };
        let mut rows: HashMap<Ref, ModelCost> = HashMap::new();
        for (referent, cost) in self.breakdowns() {
            let entry = &self.entries[&referent];
            let owner = entry.model.filter(|model| self.models.contains_key(model));
            let row = rows
                .entry(owner.unwrap_or(referent))
                .or_insert_with(|| ModelCost {
                    referent: owner.unwrap_or(referent),
                    name: owner.map_or_else(|| entry.name.clone(), |m| self.models[&m].clone()),
                    cost: Breakdown::default(),
                });
            row.cost.add(&cost);
        }
        let mut rows: Vec<ModelCost> = rows
            .into_values()
            .filter(|row| score(&row.cost) > 0.0)
            .collect();
        rows.sort_by(|a, b| {
            score(&b.cost)
                .total_cmp(&score(&a.cost))
                .then(a.referent.cmp(&b.referent))
        });
        rows.truncate(WORST);
        rows
    }
}

#[cfg(test)]
mod tests;
