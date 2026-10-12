use glam::{Vec2, Vec3};

use crate::cage::{fit_cages, CageError, CageTemplate};
use crate::mesh::Mesh;
use crate::rig::Part;
use crate::split::Body;
use crate::test_humanoid::{humanoid, Pose};

fn body_of(pose: Pose) -> Body {
    let h = humanoid(pose);
    let fit = crate::fit(&h.mesh, 5.2).unwrap();
    crate::split(&h.mesh, &fit).unwrap()
}

fn templates_of(body: &Body) -> Vec<CageTemplate> {
    Part::ALL
        .into_iter()
        .map(|part| {
            let pm = body.part(part);
            let keep = pm.mesh.triangles.len() - pm.cap_triangles;
            let mut mesh = pm.mesh.clone();
            mesh.triangles.truncate(keep);
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

/// The closest surface point to `v` and the face normal there.
fn closest(tris: &[[Vec3; 3]], v: Vec3) -> (Vec3, Vec3) {
    let mut best = (f32::MAX, Vec3::ZERO, Vec3::Y);
    for [a, b, c] in tris {
        let s = closest_on_triangle(v, *a, *b, *c);
        let d = v.distance_squared(s);
        if d < best.0 {
            let n = (*b - *a).cross(*c - *a).normalize_or_zero();
            best = (d, s, n);
        }
    }
    (best.1, best.2)
}

fn fitted(body: &Body, templates: &[CageTemplate]) -> Vec<Mesh> {
    fit_cages(body, templates).unwrap()
}

#[test]
fn keeps_template_topology() {
    let a = body_of(Pose::A);
    let templates = templates_of(&a);
    let cages = fitted(&body_of(Pose::T), &templates);
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
    let cages = fitted(&t, &templates);
    let tris = surface(&t);
    for (pm, cage) in t.parts.iter().zip(&cages) {
        for p in &cage.positions {
            let v = pm.centre + *p;
            let (s, n) = closest(&tris, v);
            assert!((v - s).dot(n) > 0.005, "{} vertex inside body", pm.part);
        }
    }
}

#[test]
fn seams_stay_welded() {
    let a = body_of(Pose::A);
    let templates = templates_of(&a);
    let t = body_of(Pose::T);
    let cages = fitted(&t, &templates);
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
    let cages = fitted(&t, &templates);
    let tris = surface(&t);
    let mut sum = 0.0;
    let mut count = 0usize;
    for (pm, cage) in t.parts.iter().zip(&cages) {
        for p in &cage.positions {
            let v = pm.centre + *p;
            sum += v.distance(closest(&tris, v).0);
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
    let cages = fitted(&a, &templates);
    for (cage, t) in cages.iter().zip(&templates) {
        for (p, q) in cage.positions.iter().zip(&t.mesh.positions) {
            assert!(p.distance(*q) < 0.1, "{} moved {}", t.part, p.distance(*q));
        }
    }
}
