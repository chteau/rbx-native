use super::*;
use glam::{Vec2, Vec3};

const SEGS: usize = 48;
const RINGS: usize = 64;

fn sphere(radius: f32) -> Mesh {
    let mut m = Mesh::default();
    m.positions.push(Vec3::Y * radius);
    m.uvs.push(Vec2::new(0.5, 0.0));
    m.positions.push(-Vec3::Y * radius);
    m.uvs.push(Vec2::new(0.5, 1.0));
    for r in 1..RINGS {
        let phi = std::f32::consts::PI * r as f32 / RINGS as f32;
        for s in 0..SEGS {
            let th = std::f32::consts::TAU * s as f32 / SEGS as f32;
            m.positions
                .push(Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin()) * radius);
            m.uvs
                .push(Vec2::new(s as f32 / SEGS as f32, r as f32 / RINGS as f32));
        }
    }
    let at = |r: usize, s: usize| (2 + (r - 1) * SEGS + s % SEGS) as u32;
    for s in 0..SEGS {
        m.triangles.push([0, at(1, s + 1), at(1, s)]);
        m.triangles
            .push([1, at(RINGS - 1, s), at(RINGS - 1, s + 1)]);
        for r in 1..RINGS - 1 {
            m.triangles.push([at(r, s), at(r, s + 1), at(r + 1, s)]);
            m.triangles
                .push([at(r, s + 1), at(r + 1, s + 1), at(r + 1, s)]);
        }
    }
    m
}

fn upper_item() -> Mesh {
    Mesh {
        positions: sphere(1.2)
            .positions
            .into_iter()
            .filter(|p| p.y >= 0.0)
            .collect(),
        ..Mesh::default()
    }
}

#[test]
fn outer_clears_item_and_keeps_far_vertices() {
    let template = sphere(1.0);
    let cages = item_cages(&upper_item(), &template);
    assert_eq!(cages.inner.positions, template.positions);
    assert_eq!(cages.outer.positions.len(), template.positions.len());
    assert_eq!(cages.outer.uvs, template.uvs);
    assert_eq!(cages.outer.triangles, template.triangles);
    for (o, t) in cages.outer.positions.iter().zip(&template.positions) {
        if t.y > 0.2 {
            assert!(o.length() >= 1.2, "{o}");
        }
        if t.y < -0.5 {
            assert!(o.distance(*t) < 1e-3);
        }
        assert!(o.distance(*t) <= REACH + MARGIN + 1e-4);
    }
}

#[test]
fn nudge_moves_nearest_and_spares_far() {
    let template = sphere(1.0);
    let mut cage = template.clone();
    let normals = smooth_normals(&cage.positions, &cage.triangles);
    nudge(&mut cage, Vec3::Y, 0.5, 0.1);
    assert!((cage.positions[0] - (template.positions[0] + normals[0] * 0.1)).length() < 1e-5);
    for (i, p) in template.positions.iter().enumerate() {
        if p.distance(Vec3::Y) >= 0.5 {
            assert_eq!(cage.positions[i], *p);
        }
    }
    assert_eq!(cage.uvs, template.uvs);
    assert_eq!(cage.triangles, template.triangles);
}

#[test]
fn zero_amount_is_noop() {
    let template = sphere(1.0);
    let mut cage = template.clone();
    nudge(&mut cage, Vec3::Y, 0.5, 0.0);
    assert_eq!(cage, template);
}

#[test]
fn seam_copies_of_a_vertex_move_as_one() {
    // Two open halves of a sphere meeting at the equator, each with its own
    // copy of the seam ring, like two body part cages put together.
    let whole = sphere(1.0);
    let mut halves = Mesh::default();
    for upper in [true, false] {
        let base = halves.positions.len() as u32;
        let keep: Vec<[u32; 3]> = whole
            .triangles
            .iter()
            .copied()
            .filter(|t| {
                let y: f32 = t.iter().map(|&i| whole.positions[i as usize].y).sum();
                (y >= 0.0) == upper
            })
            .collect();
        halves.positions.extend(whole.positions.iter().copied());
        halves.uvs.extend(whole.uvs.iter().copied());
        halves
            .triangles
            .extend(keep.iter().map(|t| t.map(|i| i + base)));
    }
    let item = sphere(1.2);
    let cages = item_cages(&item, &halves);
    let n = whole.positions.len();
    for i in 0..n {
        let (a, b) = (cages.outer.positions[i], cages.outer.positions[i + n]);
        assert!(a.distance(b) < 1e-4, "vertex {i}: {a} vs {b}");
    }
}

#[test]
fn a_sleeve_does_not_tent_the_body_beside_it() {
    // A torso and an arm 0.6 studs apart, the item a sleeve round the arm
    // only. The torso's side faces the sleeve along its normals, but the
    // sleeve is the arm's: the torso keeps its shape.
    let torso = sphere(1.0);
    let offset = Vec3::X * 2.2;
    let mut template = torso.clone();
    let base = template.positions.len() as u32;
    template
        .positions
        .extend(sphere(0.6).positions.iter().map(|p| *p + offset));
    template
        .uvs
        .extend(sphere(0.6).uvs.iter().map(|uv| *uv + Vec2::X));
    template
        .triangles
        .extend(sphere(0.6).triangles.iter().map(|t| t.map(|i| i + base)));
    let sleeve = Mesh {
        positions: sphere(0.75).positions.iter().map(|p| *p + offset).collect(),
        ..Mesh::default()
    };
    let cages = item_cages(&sleeve, &template);
    for i in 0..torso.positions.len() {
        let moved = cages.outer.positions[i].distance(template.positions[i]);
        assert!(moved < 0.05, "torso vertex {i} moved {moved}");
    }
    let arms = cages.outer.positions.iter().enumerate().take(template.positions.len());
    for (i, position) in arms.skip(torso.positions.len()) {
        let out = position.distance(offset);
        assert!(out >= 0.75, "arm vertex {i} only {out} out");
    }
}
