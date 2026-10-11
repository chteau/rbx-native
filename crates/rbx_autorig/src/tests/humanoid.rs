//! A synthetic humanoid with known joints: boxes and tapered capsules laid
//! out on the Mannequin's proportions, each a closed mesh of its own (so the
//! whole is a union of intersecting closed shells, as many real files are).
//! Built in Roblox space at height 1 (+Y up, facing −Z, left on −X), then
//! turned and scaled into whatever file space a test wants.

use std::f32::consts::{FRAC_PI_2, PI};

use glam::{Vec2, Vec3};

use crate::{Joint, Mesh, Orientation};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pose {
    /// Arms 40° from vertical, legs a little apart.
    A,
    /// Arms horizontal.
    T,
    /// Arms 18° from vertical, close to the body.
    I,
}

pub struct Humanoid {
    /// Roblox space, height 1.
    pub mesh: Mesh,
    pub joints: [Vec3; 15],
    /// The separate closed shells `mesh` is made of.
    pub shells: Vec<Mesh>,
}

/// Rings of (offset along the axis, radius) around `a`→`b`, closed with a
/// pole at each end.
fn revolve(a: Vec3, b: Vec3, profile: &[(f32, f32)], around: usize) -> Mesh {
    let axis = (b - a).try_normalize().unwrap_or(Vec3::Y);
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let mut mesh = Mesh::default();
    let mut rings: Vec<Vec<u32>> = Vec::new();
    for &(along, radius) in profile {
        let centre = a + axis * along;
        let start = mesh.positions.len() as u32;
        if radius <= 0. {
            mesh.positions.push(centre);
            mesh.normals.push(if along <= 0. { -axis } else { axis });
            mesh.uvs.push(Vec2::new(0.5, along));
            rings.push(vec![start]);
            continue;
        }
        let mut ring = Vec::with_capacity(around);
        for k in 0..around {
            let t = 2. * PI * k as f32 / around as f32;
            let dir = u * t.cos() + v * t.sin();
            mesh.positions.push(centre + dir * radius);
            mesh.normals.push(dir);
            mesh.uvs.push(Vec2::new(k as f32 / around as f32, along));
            ring.push(start + k as u32);
        }
        rings.push(ring);
    }
    for pair in rings.windows(2) {
        let (r0, r1) = (&pair[0], &pair[1]);
        match (r0.len(), r1.len()) {
            (1, n) => {
                for k in 0..n {
                    mesh.triangles.push([r0[0], r1[(k + 1) % n], r1[k]]);
                }
            }
            (n, 1) => {
                for k in 0..n {
                    mesh.triangles.push([r0[k], r0[(k + 1) % n], r1[0]]);
                }
            }
            (n, _) => {
                for k in 0..n {
                    let (a0, a1, b0, b1) = (r0[k], r0[(k + 1) % n], r1[k], r1[(k + 1) % n]);
                    mesh.triangles.push([a0, a1, b1]);
                    mesh.triangles.push([a0, b1, b0]);
                }
            }
        }
    }
    mesh
}

/// A capsule from `a` (radius `ra`) to `b` (radius `rb`): two hemispheres
/// joined by a cone.
pub fn capsule(a: Vec3, b: Vec3, ra: f32, rb: f32, step: f32) -> Mesh {
    let len = (b - a).length();
    let k = 5;
    let mut profile = vec![(-ra, 0.)];
    for i in 1..=k {
        let phi = -FRAC_PI_2 + FRAC_PI_2 * i as f32 / k as f32;
        profile.push((ra * phi.sin(), ra * phi.cos()));
    }
    let m = ((len / step).ceil() as usize).max(1);
    for i in 1..m {
        let t = i as f32 / m as f32;
        profile.push((len * t, ra + (rb - ra) * t));
    }
    for i in 0..k {
        let phi = FRAC_PI_2 * i as f32 / k as f32;
        let ring = (len + rb * phi.sin(), rb * phi.cos());
        if profile.last() != Some(&ring) {
            profile.push(ring);
        }
    }
    profile.push((len + rb, 0.));
    revolve(a, b, &profile, around(step))
}

/// Segments round a limb: 12 at the default step, more for finer steps.
fn around(step: f32) -> usize {
    ((12. * 0.025 / step).round() as usize).max(8)
}

pub fn sphere(c: Vec3, r: f32) -> Mesh {
    let k = 10;
    let profile: Vec<(f32, f32)> = (0..=k)
        .map(|i| {
            let phi = -FRAC_PI_2 + PI * i as f32 / k as f32;
            (
                r * phi.sin(),
                if i == 0 || i == k { 0. } else { r * phi.cos() },
            )
        })
        .collect();
    revolve(c, c + Vec3::Y, &profile, around(0.025))
}

/// A box with centre `c`, axes `axes` (unit, right-handed) and half sizes
/// `half`, every face a grid of about `step` cells. Faces carry their own
/// (flat-normal) vertices; shared edges get bit-identical positions.
pub fn oriented_box(c: Vec3, axes: [Vec3; 3], half: Vec3, step: f32) -> Mesh {
    let n = [0, 1, 2].map(|k| ((2. * half[k] / step).ceil() as usize).max(1));
    // The end coordinates are exactly ±half so neighbouring faces share
    // bit-identical edge vertices.
    let coord = |k: usize, i: usize| match i {
        0 => -half[k],
        i if i == n[k] => half[k],
        i => -half[k] + 2. * half[k] * (i as f32 / n[k] as f32),
    };
    let at = |q: [f32; 3]| c + axes[0] * q[0] + axes[1] * q[1] + axes[2] * q[2];
    let mut mesh = Mesh::default();
    for k in 0..3 {
        for sign in [-1f32, 1.] {
            // Face perpendicular to axis k; (a, b) spans it so a × b = sign·axis k.
            let (a, b) = if sign > 0. {
                ((k + 1) % 3, (k + 2) % 3)
            } else {
                ((k + 2) % 3, (k + 1) % 3)
            };
            let base = mesh.positions.len() as u32;
            for j in 0..=n[b] {
                for i in 0..=n[a] {
                    let mut q = [0.; 3];
                    q[k] = sign * half[k];
                    q[a] = coord(a, i);
                    q[b] = coord(b, j);
                    mesh.positions.push(at(q));
                    mesh.normals.push(axes[k] * sign);
                    mesh.uvs
                        .push(Vec2::new(i as f32 / n[a] as f32, j as f32 / n[b] as f32));
                }
            }
            let w = n[a] as u32 + 1;
            for j in 0..n[b] as u32 {
                for i in 0..n[a] as u32 {
                    let p = base + j * w + i;
                    mesh.triangles.push([p, p + 1, p + w + 1]);
                    mesh.triangles.push([p, p + w + 1, p + w]);
                }
            }
        }
    }
    mesh
}

pub fn aabb(lo: Vec3, hi: Vec3, step: f32) -> Mesh {
    oriented_box(
        (lo + hi) / 2.,
        [Vec3::X, Vec3::Y, Vec3::Z],
        (hi - lo) / 2.,
        step,
    )
}

pub fn humanoid(pose: Pose) -> Humanoid {
    humanoid_with(pose, 0.025)
}

/// The humanoid with its surfaces divided about every `step` (of the height).
pub fn humanoid_with(pose: Pose, step: f32) -> Humanoid {
    let (arm_angle, leg_angle) = match pose {
        Pose::A => (40f32, 4f32),
        Pose::T => (90., 0.),
        Pose::I => (18., 0.),
    };
    let mut joints = [Vec3::ZERO; 15];
    let mut set = |j: Joint, p: Vec3| joints[j as usize] = p;
    set(Joint::Neck, Vec3::new(0., 0.844, 0.));
    set(Joint::Waist, Vec3::new(0., 0.602, 0.));
    set(Joint::Root, Vec3::new(0., 0.533, 0.));
    let mut shells = vec![
        sphere(Vec3::new(0., 0.94, 0.), 0.06),
        capsule(
            Vec3::new(0., 0.80, 0.),
            Vec3::new(0., 0.90, 0.),
            0.03,
            0.03,
            step,
        ),
        aabb(
            Vec3::new(-0.085, 0.47, -0.06),
            Vec3::new(0.085, 0.844, 0.06),
            step,
        ),
    ];
    for side in [-1f32, 1.] {
        let (s, e, w, hp, k, a) = if side < 0. {
            (
                Joint::LeftShoulder,
                Joint::LeftElbow,
                Joint::LeftWrist,
                Joint::LeftHip,
                Joint::LeftKnee,
                Joint::LeftAnkle,
            )
        } else {
            (
                Joint::RightShoulder,
                Joint::RightElbow,
                Joint::RightWrist,
                Joint::RightHip,
                Joint::RightKnee,
                Joint::RightAnkle,
            )
        };
        let t = arm_angle.to_radians();
        let dir = Vec3::new(side * t.sin(), -t.cos(), 0.);
        let shoulder = Vec3::new(side * 0.087, 0.789, 0.);
        let elbow = shoulder + dir * 0.1425;
        let wrist = elbow + dir * 0.141;
        set(s, shoulder);
        set(e, elbow);
        set(w, wrist);
        shells.push(capsule(shoulder, elbow, 0.03, 0.026, step));
        // Not quite the upper arm's radius: identical rings would weld the two
        // shells into one non-manifold surface.
        shells.push(capsule(elbow, wrist, 0.0255, 0.018, step));
        shells.push(oriented_box(
            wrist + dir * 0.05,
            [dir, Vec3::Z.cross(dir).normalize(), Vec3::Z],
            Vec3::new(0.05, 0.015, 0.0275),
            step,
        ));

        let l = leg_angle.to_radians();
        let down = Vec3::new(side * l.sin(), -l.cos(), 0.);
        let hip = Vec3::new(side * 0.059, 0.511, 0.);
        let knee = hip + down * 0.219;
        let ankle = knee + down * 0.232;
        set(hp, hip);
        set(k, knee);
        set(a, ankle);
        shells.push(capsule(hip, knee, 0.042, 0.036, step));
        shells.push(capsule(knee, ankle, 0.0355, 0.024, step));
        shells.push(aabb(
            Vec3::new(ankle.x - 0.028, 0., ankle.z - 0.12),
            Vec3::new(ankle.x + 0.028, 0.075, ankle.z + 0.035),
            step,
        ));
    }
    let mut mesh = Mesh::default();
    for s in &shells {
        mesh.append(s.clone());
    }
    Humanoid {
        mesh,
        joints,
        shells,
    }
}

/// `mesh` (Roblox space) as a file turned by `orientation`, in `units` per
/// stud: the inverse of what fitting undoes.
pub fn to_file(mesh: &Mesh, orientation: Orientation, units: f32) -> Mesh {
    let back = orientation.rotation().transpose();
    Mesh {
        positions: mesh.positions.iter().map(|&p| back * p * units).collect(),
        normals: mesh.normals.iter().map(|&n| back * n).collect(),
        ..mesh.clone()
    }
}
