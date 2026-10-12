use glam::{Vec2, Vec3};

use crate::cage::{fit_cages, fit_cages_report, CageError, CageTemplate};
use crate::mesh::{smooth_normals, weld, Mesh};
use crate::rig::Part;
use crate::split::Body;
use crate::test_humanoid::{humanoid, Pose};

fn body_of(pose: Pose) -> Body {
    let h = humanoid(pose);
    let fit = crate::fit(&h.mesh, 5.2).unwrap();
    crate::split(&h.mesh, &fit).unwrap()
}

/// Template cages: each part's mesh without its caps, pushed out along normals
/// computed over the welded body so seam copies move identically.
fn templates_of(body: &Body) -> Vec<CageTemplate> {
    let mut world = Vec::new();
    let mut offsets = Vec::new();
    for pm in &body.parts {
        offsets.push(world.len());
        world.extend(pm.mesh.positions.iter().map(|p| pm.centre + *p));
    }
    let welded = weld(&world, 1e-4);
    let mut tris = Vec::new();
    for (pm, off) in body.parts.iter().zip(&offsets) {
        for t in &pm.mesh.triangles {
            tris.push(t.map(|i| welded.of[off + i as usize]));
        }
    }
    let normals = smooth_normals(&welded.positions, &tris);
    let solids = solids(body);

    Part::ALL
        .into_iter()
        .map(|part| {
            let pm = body.part(part);
            let off = offsets[part.index()];
            let keep = pm.mesh.triangles.len() - pm.cap_triangles;
            // The humanoid's shells overlap (the neck runs up into the head):
            // a cage is the outside of the body only, so faces buried in
            // another part go, and the vertices only they used with them.
            let buried = |i: u32| {
                let at = pm.centre + pm.mesh.positions[i as usize];
                solids
                    .iter()
                    .enumerate()
                    .any(|(j, s)| j != part.index() && winding_number(s, at) > 0.5)
            };
            let faces: Vec<[u32; 3]> = pm.mesh.triangles[..keep]
                .iter()
                .copied()
                .filter(|t| !t.iter().any(|&i| buried(i)))
                .collect();
            let mut remap = vec![u32::MAX; pm.mesh.positions.len()];
            let mut mesh = Mesh::default();
            for t in &faces {
                for &i in t {
                    if remap[i as usize] == u32::MAX {
                        remap[i as usize] = mesh.positions.len() as u32;
                        mesh.positions.push(
                            pm.mesh.positions[i as usize]
                                + normals[welded.of[off + i as usize] as usize] * 0.04,
                        );
                        if let Some(uv) = pm.mesh.uvs.get(i as usize) {
                            mesh.uvs.push(*uv);
                        }
                    }
                }
            }
            mesh.triangles = faces.iter().map(|t| t.map(|i| remap[i as usize])).collect();
            if mesh.uvs.is_empty() {
                mesh.uvs = (0..mesh.positions.len())
                    .map(|i| Vec2::new(i as f32 * 0.001, part.index() as f32))
                    .collect();
            }
            CageTemplate {
                part,
                mesh,
                size: pm.size,
                attachments: pm.attachments.clone(),
            }
        })
        .collect()
}

fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// World-space non-cap triangles of the body.
fn surface(body: &Body) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    for pm in &body.parts {
        let keep = pm.mesh.triangles.len() - pm.cap_triangles;
        for t in &pm.mesh.triangles[..keep] {
            out.push(t.map(|i| pm.centre + pm.mesh.positions[i as usize]));
        }
    }
    out
}

fn distance_to(tris: &[[Vec3; 3]], v: Vec3) -> f32 {
    tris.iter()
        .map(|[a, b, c]| v.distance_squared(closest_on_triangle(v, *a, *b, *c)))
        .fold(f32::MAX, f32::min)
        .sqrt()
}

/// Generalized winding number of `p` against a closed triangle soup.
fn winding_number(tris: &[[Vec3; 3]], p: Vec3) -> f32 {
    let mut sum = 0.0;
    for [a, b, c] in tris {
        let (a, b, c) = (*a - p, *b - p, *c - p);
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        let num = a.dot(b.cross(c));
        let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
        sum += 2.0 * num.atan2(den);
    }
    sum / (4.0 * std::f32::consts::PI)
}

/// World-space triangles of each part's full closed mesh, caps included.
fn solids(body: &Body) -> Vec<Vec<[Vec3; 3]>> {
    body.parts
        .iter()
        .map(|pm| {
            pm.mesh
                .triangles
                .iter()
                .map(|t| t.map(|i| pm.centre + pm.mesh.positions[i as usize]))
                .collect()
        })
        .collect()
}

#[test]
fn keeps_template_topology() {
    let templates = templates_of(&body_of(Pose::A));
    let cages = fit_cages(&body_of(Pose::T), &templates).unwrap();
    assert_eq!(cages.len(), 15);
    for (cage, t) in cages.iter().zip(&templates) {
        assert_eq!(cage.triangles, t.mesh.triangles, "{}", t.part);
        assert_eq!(cage.uvs, t.mesh.uvs, "{}", t.part);
        assert_eq!(cage.positions.len(), t.mesh.positions.len());
        assert_eq!(cage.normals.len(), cage.positions.len());
        assert!(cage.positions.iter().all(|p| p.is_finite()));
        assert!(cage.normals.iter().all(|n| n.is_finite()));
    }
}

#[test]
fn cage_stays_outside_body() {
    let templates = templates_of(&body_of(Pose::A));
    let t = body_of(Pose::T);
    let cages = fit_cages(&t, &templates).unwrap();
    let solids = solids(&t);
    for (pm, cage) in t.parts.iter().zip(&cages) {
        for p in &cage.positions {
            let v = pm.centre + *p;
            for (k, solid) in solids.iter().enumerate() {
                let w = winding_number(solid, v);
                assert!(
                    w <= 0.5,
                    "{} cage vertex inside {} (winding {w})",
                    pm.part,
                    t.parts[k].part
                );
            }
        }
    }
}

#[test]
fn seams_stay_welded() {
    let a = body_of(Pose::A);
    let templates = templates_of(&a);
    let t = body_of(Pose::T);
    let cages = fit_cages(&t, &templates).unwrap();
    let mut pairs = 0;
    for i in 0..15 {
        for j in (i + 1)..15 {
            for (vi, pi) in templates[i].mesh.positions.iter().enumerate() {
                let wi = a.parts[i].centre + *pi;
                for (vj, pj) in templates[j].mesh.positions.iter().enumerate() {
                    let wj = a.parts[j].centre + *pj;
                    if wi.distance(wj) < 1e-4 {
                        pairs += 1;
                        let fi = t.parts[i].centre + cages[i].positions[vi];
                        let fj = t.parts[j].centre + cages[j].positions[vj];
                        assert!(
                            fi.distance(fj) < 1e-3,
                            "seam {} / {} split apart",
                            templates[i].part,
                            templates[j].part
                        );
                    }
                }
            }
        }
    }
    assert!(pairs >= 50, "only {pairs} seam pairs");
}

#[test]
fn cage_hugs_body() {
    let templates = templates_of(&body_of(Pose::A));
    let t = body_of(Pose::T);
    let cages = fit_cages(&t, &templates).unwrap();
    let tris = surface(&t);
    let mut sum = 0.0;
    let mut count = 0usize;
    for (pm, cage) in t.parts.iter().zip(&cages) {
        for p in &cage.positions {
            sum += distance_to(&tris, pm.centre + *p);
            count += 1;
        }
    }
    let mean = sum / count as f32;
    assert!(mean < 0.15, "mean distance {mean}");
}

#[test]
fn missing_template_is_an_error() {
    let a = body_of(Pose::A);
    let mut templates = templates_of(&a);
    templates.retain(|t| t.part != Part::LeftHand);
    assert_eq!(
        fit_cages(&a, &templates),
        Err(CageError::MissingTemplate(Part::LeftHand))
    );
}

#[test]
fn fitting_onto_own_body_barely_moves() {
    let a = body_of(Pose::A);
    let templates = templates_of(&a);
    let cages = fit_cages(&a, &templates).unwrap();
    // The humanoid's shells overlap (the neck runs up into the head), so a
    // template made of its surface starts partly inside the body, and those
    // points have to move out; only the ones that start outside must stay.
    // Untangling the folds where the shells cross smooths a few points
    // further, so the bound is on almost all of them, not every one.
    let solids = solids(&a);
    let (mut checked, mut total) = (0, 0);
    let mut moves = Vec::new();
    for (cage, t) in cages.iter().zip(&templates) {
        let centre = a.part(t.part).centre;
        for (p, q) in cage.positions.iter().zip(&t.mesh.positions) {
            total += 1;
            let world = centre + *q;
            if solids.iter().any(|s| winding_number(s, world) > 0.5) {
                continue;
            }
            checked += 1;
            moves.push(p.distance(*q));
        }
    }
    assert!(
        checked * 10 >= total * 7,
        "only {checked} of {total} start outside"
    );
    moves.sort_by(f32::total_cmp);
    let p95 = moves[moves.len() * 95 / 100];
    let max = moves[moves.len() - 1];
    assert!(
        p95 < 0.05 && max < 0.25,
        "95% moved under {p95}, the most {max}"
    );
}

#[test]
fn folded_triangles_are_rare() {
    let a = body_of(Pose::A);
    let templates = templates_of(&a);
    let total: usize = templates.iter().map(|t| t.mesh.triangles.len()).sum();

    let (_, folded) = fit_cages_report(&a, &templates).unwrap();
    // The synthetic template folds where the humanoid's shells cut into
    // each other (neck into head, arms into the chest): its offset surface
    // crosses itself there before any fitting. Real Mannequin cages on real
    // characters fold far less (the ignored test in rbx_studio's
    // `cage_templates`); these bounds guard against the fitter regressing.
    let own: usize = folded.iter().sum();
    assert!(own * 100 <= total * 3, "A onto A: {own} of {total}");

    let (_, folded) = fit_cages_report(&body_of(Pose::T), &templates).unwrap();
    let t_folded: usize = folded.iter().sum();
    assert!(
        t_folded * 100 <= total * 4,
        "A onto T: {t_folded} of {total}"
    );
}
