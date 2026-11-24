//! How a layered item's render mesh and outer cage sit on each other:
//! Studio's `Measure_Mesh_Outside_OuterCage` and `Measure_Cage_Mesh_Distance`.
//! The limits are the defaults the validation-system page gives; the
//! messages are the ones Studio 0.748 shows.
//!
//! Against Studio on Roblox's leather jacket (7192549218), its outer cage
//! pushed 0 to 1 stud forward, the share of the mesh outside comes out
//! within 1.5 points and the farthest cage vertex to the thousandth. The
//! share of cage vertices inside the mesh comes out lower than Studio's
//! (23.9% where it says 32.8% at a quarter stud): it counts a set of
//! vertices we haven't pinned down.

use std::collections::HashMap;

use super::distance::Surface;
use crate::{Affine, Cage, CageKind, Category, Finding, Geometry, Item, Rule, Severity, Value};

/// Percent of render-mesh vertices outside the outer cage above which the
/// item fails.
const OUTSIDE_LIMIT: f32 = 65.0;
/// Farthest a moved outer-cage vertex may sit from the render mesh.
const DISTANCE_LIMIT: f32 = 0.6;
/// Percent of moved outer-cage vertices inside the render mesh above which
/// the item fails.
const INSIDE_LIMIT: f32 = 30.0;
/// How far an outer-cage vertex must sit from the inner cage's to count as
/// moved. Not published: on Roblox's leather jacket (7192549218), with its
/// outer cage pushed 0, 0.05, 0.1, 0.25, 0.5 and 1 stud forward, Studio
/// 0.748's farthest cage vertex matches ours to the thousandth for any
/// value from 0.065 to 0.09, and for none below.
const MOVED: f32 = 0.075;
/// The grid the files' offset is found on.
const GRID: f32 = 1e-4;

fn apply(m: &Affine, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] + m[3 + 3 * r] * p[0] + m[4 + 3 * r] * p[1] + m[5 + 3 * r] * p[2])
}

/// The handle's name and mesh and the outer cage of a layered item, when
/// it has both.
fn pair(item: &Item) -> Option<(&str, &Geometry, &Cage)> {
    if item.asset_type.category() != Category::Layered {
        return None;
    }
    let (name, mesh) = item
        .parts
        .iter()
        .find_map(|p| Some((p.name.as_str(), p.geometry.as_ref()?)))?;
    let cage = item.cages.iter().find(|c| c.kind == CageKind::Outer)?;
    (!mesh.positions.is_empty() && !cage.geometry.positions.is_empty())
        .then_some((name, mesh, cage))
}

/// The render-mesh vertices of a layered item that sit outside its outer
/// cage, by index; none when it lacks either.
pub fn mesh_outside_cage(item: &Item) -> Vec<usize> {
    let Some((_, mesh, cage)) = pair(item) else {
        return Vec::new();
    };
    let surface = Surface::new(&cage.geometry.positions, &cage.geometry.triangles, |p| {
        apply(&cage.to_mesh, p)
    });
    if surface.is_empty() {
        return Vec::new();
    }
    // One test per welded corner: a seam's copies share the answer.
    let welded = super::weld(&mesh.positions);
    let mut outside: HashMap<u32, bool> = HashMap::new();
    mesh.positions
        .iter()
        .zip(&welded)
        .enumerate()
        .filter(|(_, (p, id))| {
            *outside
                .entry(**id)
                .or_insert_with(|| !surface.encloses(**p))
        })
        .map(|(i, _)| i)
        .collect()
}

/// The share of `mesh`'s welded corners `outside` names, in percent.
fn welded_share(mesh: &Geometry, outside: &[usize]) -> f32 {
    let welded = super::weld(&mesh.positions);
    let all = welded.iter().copied().max().map_or(0, |m| m as usize + 1);
    let out: std::collections::HashSet<u32> = outside.iter().map(|&i| welded[i]).collect();
    100.0 * out.len() as f32 / all.max(1) as f32
}

fn uv_key(uv: [f32; 2]) -> [i64; 2] {
    uv.map(|c| (c * 1e5).round() as i64)
}

/// The outer-cage vertices the creator moved off the inner cage. The two
/// files order their vertices differently, so vertices pair up by UV (the
/// cage UVs stay put; copies on a seam share a position). Each file is
/// also centred on its own, so the offset most pairs share is the file
/// offset, not a move. Every vertex when there is no inner cage to compare
/// with.
fn moved(item: &Item, outer: &Geometry) -> Vec<usize> {
    let inner = item
        .cages
        .iter()
        .find(|c| c.kind == CageKind::Inner)
        .map(|c| &c.geometry)
        .filter(|g| !g.uvs.is_empty() && !outer.uvs.is_empty());
    let Some(inner) = inner else {
        return (0..outer.positions.len()).collect();
    };
    let by_uv: HashMap<[i64; 2], [f32; 3]> = inner
        .uvs
        .iter()
        .zip(&inner.positions)
        .map(|(uv, p)| (uv_key(*uv), *p))
        .collect();
    let diffs: Vec<Option<[f32; 3]>> = outer
        .uvs
        .iter()
        .zip(&outer.positions)
        .map(|(uv, p)| {
            let q = by_uv.get(&uv_key(*uv))?;
            Some(std::array::from_fn(|k| p[k] - q[k]))
        })
        .collect();
    let mut counts: HashMap<[i64; 3], usize> = HashMap::new();
    for d in diffs.iter().flatten() {
        *counts
            .entry(d.map(|c| (c / GRID).round() as i64))
            .or_insert(0) += 1;
    }
    // An offset only one pair has is a move, not the files' offset.
    let offset = counts
        .into_iter()
        .filter(|&(_, n)| n > 1)
        .max_by_key(|&(key, n)| (n, std::cmp::Reverse(key)))
        .map_or([0.0; 3], |(key, _)| key.map(|c| c as f32 * GRID));
    diffs
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.is_none_or(|d| {
                let off: [f32; 3] = std::array::from_fn(|k| d[k] - offset[k]);
                super::distance::length(off) > MOVED
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// Each moved outer-cage vertex (by index) and its signed distance to the
/// render mesh, in the mesh's units, negative behind it; empty when the
/// item lacks either. Studio measures only the vertices the creator moved:
/// the rest of the body cage is nowhere near the garment.
pub fn cage_mesh_distances(item: &Item) -> Vec<(usize, f32)> {
    let Some((_, mesh, cage)) = pair(item) else {
        return Vec::new();
    };
    let surface = Surface::new(&mesh.positions, &mesh.triangles, |p| p);
    if surface.is_empty() {
        return Vec::new();
    }
    moved(item, &cage.geometry)
        .into_iter()
        .map(|i| {
            let p = apply(&cage.to_mesh, cage.geometry.positions[i]);
            (i, surface.signed_distance(p))
        })
        .collect()
}

/// The outer-cage vertices [`cage_mesh_distances`] counts against the
/// item: inside the render mesh or too far from it.
pub fn cage_distance_failures(distances: &[(usize, f32)]) -> Vec<usize> {
    distances
        .iter()
        .filter(|(_, d)| *d < 0.0 || *d > DISTANCE_LIMIT)
        .map(|(i, _)| *i)
        .collect()
}

fn finding(rule: Rule, severity: Severity, subject: &str, message: String) -> Finding {
    Finding {
        rule,
        severity,
        subject: subject.to_string(),
        measured: None,
        limit: None,
        message,
    }
}

pub(crate) fn fit(item: &Item, out: &mut Vec<Finding>) {
    let Some((handle, mesh, cage)) = pair(item) else {
        return;
    };

    let outside = mesh_outside_cage(item);
    let outside_share = welded_share(mesh, &outside);
    let share = super::percent(outside_share);
    if outside_share > OUTSIDE_LIMIT {
        out.push(Finding {
            measured: Some(Value::Text(share.clone())),
            limit: Some(Value::Text(super::percent(OUTSIDE_LIMIT))),
            ..finding(
                Rule::MeshOutsideCage,
                Severity::Error,
                handle,
                format!("{share} of mesh vertices for the accessory are outside its outer cage. Make adjustments to have more of the accessory placed between the cages."),
            )
        });
    }
    if !outside.is_empty() {
        out.push(finding(
            Rule::MeshOutsideCage,
            Severity::Warning,
            handle,
            format!("{share} of mesh vertices are outside the outer cage. Please adjust the outer cage to fully contain the render mesh."),
        ));
    }

    let distances = cage_mesh_distances(item);
    if distances.is_empty() {
        return;
    }
    let max = distances.iter().fold(0.0f32, |m, (_, d)| m.max(*d));
    let inside = distances.iter().filter(|(_, d)| *d < 0.0).count();
    let inside_share = 100.0 * inside as f32 / distances.len() as f32;
    let inside_text = super::percent(inside_share);
    let subject = cage.name.as_str();
    if max > DISTANCE_LIMIT {
        out.push(Finding {
            measured: Some(Value::Text(format!("{max:.3}"))),
            limit: Some(Value::Text(format!("{DISTANCE_LIMIT:.3}"))),
            ..finding(
                Rule::CageDistance,
                Severity::Error,
                subject,
                format!("Outer cage is too far from the render mesh. Maximum distance between outer cage and mesh is {:.6}, which is above allowed threshold {DISTANCE_LIMIT:.6}", (max * 1000.0).round() / 1000.0),
            )
        });
    }
    if inside_share > INSIDE_LIMIT {
        out.push(Finding {
            measured: Some(Value::Text(inside_text.clone())),
            limit: Some(Value::Text(super::percent(INSIDE_LIMIT))),
            ..finding(
                Rule::CageDistance,
                Severity::Error,
                subject,
                format!("Outer cage intersects with the render mesh. {inside_text} of outer cage vertices are inside render mesh."),
            )
        });
    }
    if inside > 0 || max > DISTANCE_LIMIT {
        out.push(finding(
            Rule::CageDistance,
            Severity::Warning,
            subject,
            format!("Outer cage does not closely follow the render mesh. Maximum distance between outer cage and mesh is {max:.3}, and {inside_text} of outer cage vertices are inside the render mesh."),
        ));
    }
}

#[cfg(test)]
#[path = "fit/tests.rs"]
mod tests;
