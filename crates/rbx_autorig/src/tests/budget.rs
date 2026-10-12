//! Simplifying each body asset down to Roblox's Marketplace triangle budget.

use std::collections::HashMap;
use std::sync::OnceLock;

use glam::Vec3;

use super::humanoid::{humanoid_with, Humanoid, Pose};
use super::{assert_watertight, HEIGHT};
use crate::split::{cut, fit_budgets, BUDGETS};
use crate::{fit, Body, Fit, PartMesh};

struct Dense {
    body: Humanoid,
    fit: Fit,
    before: Vec<PartMesh>,
    after: Vec<PartMesh>,
    warnings: Vec<String>,
}

/// A humanoid about three times as fine as the other tests', cut, then
/// simplified: every asset but the head starts over its budget.
fn dense() -> &'static Dense {
    static DENSE: OnceLock<Dense> = OnceLock::new();
    DENSE.get_or_init(|| {
        let body = humanoid_with(Pose::A, 0.016);
        let fit = fit(&body.mesh, HEIGHT).unwrap();
        let before = cut(&body.mesh, &fit, 128).unwrap();
        let mut after = before.clone();
        let warnings = fit_budgets(&mut after, &BUDGETS);
        Dense {
            body,
            fit,
            before,
            after,
            warnings,
        }
    })
}

fn asset_count(parts: &[PartMesh], members: &[crate::Part]) -> usize {
    members
        .iter()
        .map(|p| parts[p.index()].mesh.triangles.len())
        .sum()
}

/// A part's cap triangles in Roblox space.
fn caps(piece: &PartMesh) -> Vec<[Vec3; 3]> {
    let m = &piece.mesh;
    m.triangles[m.triangles.len() - piece.cap_triangles..]
        .iter()
        .map(|t| t.map(|i| piece.centre + m.positions[i as usize]))
        .collect()
}

fn world_bounds(piece: &PartMesh) -> (Vec3, Vec3) {
    crate::mesh::bounds(piece.mesh.positions.iter().map(|&p| piece.centre + p))
}

#[test]
fn every_asset_is_simplified_to_its_budget() {
    let d = dense();
    let mut over = 0;
    for (asset, members, budget) in BUDGETS {
        let before = asset_count(&d.before, members);
        let after = asset_count(&d.after, members);
        eprintln!("{asset}: {before} -> {after} (budget {budget})");
        over += usize::from(before > budget);
        assert!(after <= budget, "{asset}: {after} over {budget}");
        // Not simplified further than it had to be.
        assert!(after >= budget.min(before) * 9 / 10, "{asset}: {after}");
    }
    assert!(over >= 4, "only {over} assets started over budget");
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    for piece in &d.before {
        assert_watertight(&piece.mesh, &format!("{} as cut", piece.part.name()));
    }
    for piece in &d.after {
        let what = piece.part.name();
        assert_watertight(&piece.mesh, what);
        assert_eq!(piece.mesh.normals.len(), piece.mesh.positions.len());
        assert_eq!(piece.mesh.uvs.len(), piece.mesh.positions.len());
        let (lo, hi) = crate::mesh::bounds(piece.mesh.positions.iter().copied());
        assert!(((lo + hi) / 2.).length() < 1e-4, "{what} is not centred");
        assert!((hi - lo - piece.size).length() < 1e-4, "{what}'s size");
    }
}

#[test]
fn the_cut_seams_between_parts_are_left_as_they_were() {
    let d = dense();
    for (before, after) in d.before.iter().zip(&d.after) {
        assert_eq!(before.cap_triangles, after.cap_triangles);
        // Recentring on the smaller bounds may round the last bit.
        for (b, a) in caps(before).iter().zip(caps(after)) {
            for k in 0..3 {
                assert!(
                    (b[k] - a[k]).length() < 1e-5,
                    "{}: a cut seam moved",
                    before.part.name()
                );
            }
        }
        // Attachments stay at their joints in Roblox space.
        for ((name, b), (_, a)) in before.attachments.iter().zip(&after.attachments) {
            let moved = (before.centre + *b - (after.centre + *a)).length();
            assert!(moved < 1e-4, "{name} moved {moved}");
        }
    }
}

#[test]
fn the_shape_keeps_its_bounds() {
    let d = dense();
    for (before, after) in d.before.iter().zip(&d.after) {
        let (lo0, hi0) = world_bounds(before);
        let (lo1, hi1) = world_bounds(after);
        let off = (lo1 - lo0).abs().max((hi1 - hi0).abs()).max_element();
        assert!(
            off < 0.01 * HEIGHT,
            "{}: bounds moved {off}",
            before.part.name()
        );
    }
}

#[test]
fn kept_vertices_keep_their_uvs_and_normals_and_take_skin_weights() {
    let d = dense();
    let key = |piece: &PartMesh, i: usize| {
        let m = &piece.mesh;
        let n = m.normals[i].to_array().map(f32::to_bits);
        let uv = m.uvs[i].to_array().map(f32::to_bits);
        (n, uv)
    };
    for (before, after) in d.before.iter().zip(&d.after) {
        let mut had: HashMap<_, Vec<Vec3>> = HashMap::new();
        for i in 0..before.mesh.positions.len() {
            had.entry(key(before, i))
                .or_default()
                .push(before.centre + before.mesh.positions[i]);
        }
        for i in 0..after.mesh.positions.len() {
            let p = after.centre + after.mesh.positions[i];
            assert!(
                had.get(&key(after, i))
                    .is_some_and(|ps| ps.iter().any(|q| (*q - p).length() < 1e-5)),
                "{}: vertex {i} is new",
                after.part.name()
            );
        }
    }
    let body = Body {
        parts: d.after.clone(),
        warnings: Vec::new(),
    };
    let skin = crate::weights(&d.body.mesh, &d.fit, &body);
    for (piece, skin) in body.parts.iter().zip(&skin.parts) {
        assert_eq!(skin.influences.len(), piece.mesh.positions.len());
    }
}

#[test]
fn simplifying_is_deterministic() {
    let d = dense();
    let mut again = d.before.clone();
    fit_budgets(&mut again, &BUDGETS);
    assert!(again == d.after);
}

#[test]
fn a_body_under_budget_is_left_alone() {
    let d = dense();
    let mut parts = d.after.clone();
    assert!(fit_budgets(&mut parts, &BUDGETS).is_empty());
    assert!(parts == d.after);
}

#[test]
fn an_asset_its_seams_keep_over_budget_is_named() {
    let d = dense();
    let mut parts = d.before.clone();
    let tight: [(&str, &[crate::Part], usize); 1] = [("LeftArm", BUDGETS[2].1, 100)];
    let warnings = fit_budgets(&mut parts, &tight);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("LeftArm still has"),
        "{}",
        warnings[0]
    );
    // It still got as far as the seams allow.
    assert!(asset_count(&parts, BUDGETS[2].1) < asset_count(&d.before, BUDGETS[2].1) / 2);
}
