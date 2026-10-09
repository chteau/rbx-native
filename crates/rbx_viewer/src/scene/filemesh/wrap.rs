//! Layered clothing, wrapped to the body.
//!
//! A layered garment's `WrapLayer` names the body it was modelled on
//! (`ReferenceMeshId`, a cage in the garment's own frame, shifted by
//! `ReferenceOrigin`); each body part of the character it is worn on carries a
//! `WrapTarget` whose `CageMeshId` is the body as it is now. Roblox fits the
//! garment by cage-to-cage correspondence. Here every reference-cage vertex
//! is matched to the nearest current-cage vertex in the world, and each
//! garment vertex moves by the inverse-distance blend of the four nearest
//! reference vertices' displacements, then a little along its normal by the
//! layer's `Order`, so stacked layers do not fight over one surface. On a
//! body shaped like the reference the displacement is nil.
//!
//! The reference cage rests in the garment's frame, which need not be where the
//! body stands (the real Black Detective Trench Coat's sits 0.8 studs high), so
//! first the reference is slid onto the body ([`register`]); matching vertices
//! across that gap would pick the wrong surface and tear the garment.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::fit::{self, Fit};
use super::{parsed_asset_ref, Entry};
use crate::scene::cframe_matrix;

/// Neighbours blended per garment vertex.
const BLEND: usize = 4;
/// A body cage vertex further than this from its reference vertex is another
/// limb's, not this one's resized.
const MAX_SHIFT: f32 = 1.5;
/// Reference vertices sampled per registration pass.
const SAMPLES: usize = 600;
/// Registration passes per starting offset.
const PASSES: usize = 10;
/// The share of the closest matches that steer registration, so a sleeve the
/// body lacks does not drag the whole reference after it.
const TRIM: f32 = 0.7;
/// Studs of lift per `Order` step.
const LIFT: f32 = 0.004;

#[derive(Clone)]
pub(super) struct Wrap {
    reference: AssetRef,
    /// `ReferenceOrigin`, in the garment's mesh space.
    origin: Mat4,
    order: f32,
    targets: Vec<Target>,
}

#[derive(Clone)]
struct Target {
    cage: AssetRef,
    origin: Mat4,
    /// The body part's own mesh and fit: the cage shares its space.
    mesh: AssetRef,
    fit: Fit,
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
    let reference = parsed_asset_ref(layer.properties().get("ReferenceMeshId")?)?;
    let origin = match layer.properties().get("ReferenceOrigin") {
        Some(Variant::CFrame(c)) => cframe_matrix(c),
        _ => Mat4::IDENTITY,
    };
    let order = crate::scene::number(layer.properties().get("Order"));
    let rig = dom.parent(dom.parent(handle)?)?;
    let targets: Vec<Target> = dom
        .get(rig)?
        .children()
        .iter()
        .filter_map(|&part| {
            let part = dom.get(part)?;
            let (mesh, fit) = fit::mesh_part(database, part)?;
            let target = part
                .children()
                .iter()
                .filter_map(|&c| dom.get(c))
                .find(|c| c.class() == "WrapTarget")?;
            Some(Target {
                cage: parsed_asset_ref(target.properties().get("CageMeshId")?)?,
                origin: match target.properties().get("CageOrigin") {
                    Some(Variant::CFrame(c)) => cframe_matrix(c),
                    _ => Mat4::IDENTITY,
                },
                mesh,
                fit,
            })
        })
        .collect();
    (!targets.is_empty()).then_some(Wrap {
        reference,
        origin,
        order,
        targets,
    })
}

impl Wrap {
    /// The meshes the wrap needs besides the garment's own.
    pub(super) fn meshes(&self) -> impl Iterator<Item = &AssetRef> {
        std::iter::once(&self.reference).chain(self.targets.iter().flat_map(|t| [&t.cage, &t.mesh]))
    }
}

pub(super) fn key(referent: Ref) -> AssetRef {
    AssetRef::Thumb(format!("wrapped-mesh/{referent:?}"))
}

/// Every wrapped garment's deformed mesh, by referent, for those whose cages
/// have all arrived.
pub(super) fn derive(
    entries: &[Entry],
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
) -> HashMap<Ref, (AssetRef, Arc<rbx_mesh::Mesh>)> {
    entries
        .iter()
        .filter_map(|entry| {
            let wrap = entry.wrap.as_ref()?;
            let garment = meshes.get(&entry.mesh)?;
            let deformed = deform(wrap, entry.fit.transform(garment), garment, meshes)?;
            Some((entry.referent, (key(entry.referent), Arc::new(deformed))))
        })
        .collect()
}

fn deform(
    wrap: &Wrap,
    model: Mat4,
    garment: &rbx_mesh::Mesh,
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
) -> Option<rbx_mesh::Mesh> {
    let reference = meshes.get(&wrap.reference)?;
    let mut body = Vec::new();
    for target in &wrap.targets {
        let (cage, part) = (meshes.get(&target.cage)?, meshes.get(&target.mesh)?);
        let to_world = target.fit.transform(part) * target.origin;
        body.extend(
            cage.vertices
                .iter()
                .map(|v| to_world.transform_point3(Vec3::from(v.position))),
        );
    }
    let to_world = model * wrap.origin;
    let reference_points: Vec<Vec3> = reference
        .vertices
        .iter()
        .map(|v| to_world.transform_point3(Vec3::from(v.position)))
        .collect();
    let slide = register(&reference_points, &body);
    // Where each reference vertex stands, and how far its body counterpart is.
    let anchors: Vec<(Vec3, Vec3)> = reference_points
        .iter()
        .filter_map(|&point| {
            let at = point + slide;
            let shift = nearest(&body, at)? - at;
            (shift.length() <= MAX_SHIFT).then_some((at, shift))
        })
        .collect();
    if anchors.is_empty() {
        return None;
    }
    let back = model.inverse();
    let mut out = garment.clone();
    for vertex in &mut out.vertices {
        let at = model.transform_point3(Vec3::from(vertex.position)) + slide;
        let normal = model
            .transform_vector3(Vec3::from(vertex.normal))
            .normalize_or_zero();
        let moved = at + blended(&anchors, at) + normal * LIFT * (wrap.order + 1.0);
        vertex.position = back.transform_point3(moved).to_array();
    }
    Some(out)
}

/// The translation that lays the reference over the body: nearest-vertex
/// matching from a few starting offsets, each pass moving by the mean of the
/// closest [`TRIM`] of the gaps, and the start with the least left over wins.
fn register(reference: &[Vec3], body: &[Vec3]) -> Vec3 {
    let stride = (reference.len() / SAMPLES).max(1);
    let sample: Vec<Vec3> = reference.iter().copied().step_by(stride).collect();
    let (rb, bb) = (bounds(reference), bounds(body));
    let starts = [
        Vec3::ZERO,
        (bb.0 + bb.1 - rb.0 - rb.1) * 0.5,
        Vec3::new(0.0, bb.0.y - rb.0.y, 0.0),
        Vec3::new(0.0, bb.1.y - rb.1.y, 0.0),
    ];
    let gaps = |slide: Vec3| -> Vec<Vec3> {
        let mut gaps: Vec<Vec3> = sample
            .iter()
            .filter_map(|&p| nearest(body, p + slide).map(|n| n - p - slide))
            .collect();
        gaps.sort_by(|a, b| a.length_squared().total_cmp(&b.length_squared()));
        gaps.truncate(((gaps.len() as f32 * TRIM).ceil() as usize).max(1));
        gaps
    };
    let mean = |g: &[Vec3]| g.iter().copied().sum::<Vec3>() / g.len() as f32;
    let mut best = (f32::INFINITY, Vec3::ZERO);
    for start in starts {
        let mut slide = start;
        for _ in 0..PASSES {
            let step = mean(&gaps(slide));
            slide += step;
            if step.length_squared() < 1e-8 {
                break;
            }
        }
        let left = gaps(slide);
        let left = left.iter().map(|g| g.length()).sum::<f32>() / left.len() as f32;
        if left < best.0 - 1e-6 {
            best = (left, slide);
        }
    }
    best.1
}

fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), &p| (lo.min(p), hi.max(p)),
    )
}

fn nearest(points: &[Vec3], at: Vec3) -> Option<Vec3> {
    points
        .iter()
        .copied()
        .min_by(|a, b| a.distance_squared(at).total_cmp(&b.distance_squared(at)))
}

/// The inverse-square-distance blend of the [`BLEND`] anchors nearest `at`.
fn blended(anchors: &[(Vec3, Vec3)], at: Vec3) -> Vec3 {
    let mut near = [(f32::INFINITY, Vec3::ZERO); BLEND];
    for &(anchor, shift) in anchors {
        let d = anchor.distance_squared(at);
        if d < near[BLEND - 1].0 {
            near[BLEND - 1] = (d, shift);
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
    let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
    for (d, shift) in near.iter().filter(|(d, _)| d.is_finite()) {
        let w = 1.0 / (d + 1e-6);
        sum += *shift * w;
        weight += w;
    }
    if weight > 0.0 {
        sum / weight
    } else {
        Vec3::ZERO
    }
}

#[cfg(test)]
#[path = "wrap/tests.rs"]
mod tests;
