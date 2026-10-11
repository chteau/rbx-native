//! Which way the file is turned: its up and front axes, and the rotation that
//! takes it into Roblox space (+Y up, the character facing −Z, its left on −X).

use glam::{Mat3, Vec2, Vec3};

use crate::mesh::Mesh;
use crate::Error;

/// A signed coordinate axis of the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl Axis {
    pub const ALL: [Axis; 6] = [
        Axis::PosX,
        Axis::NegX,
        Axis::PosY,
        Axis::NegY,
        Axis::PosZ,
        Axis::NegZ,
    ];

    pub fn vector(self) -> Vec3 {
        match self {
            Axis::PosX => Vec3::X,
            Axis::NegX => Vec3::NEG_X,
            Axis::PosY => Vec3::Y,
            Axis::NegY => Vec3::NEG_Y,
            Axis::PosZ => Vec3::Z,
            Axis::NegZ => Vec3::NEG_Z,
        }
    }

    /// 0, 1 or 2 for X, Y or Z.
    pub fn component(self) -> usize {
        match self {
            Axis::PosX | Axis::NegX => 0,
            Axis::PosY | Axis::NegY => 1,
            Axis::PosZ | Axis::NegZ => 2,
        }
    }

    pub fn negate(self) -> Axis {
        match self {
            Axis::PosX => Axis::NegX,
            Axis::NegX => Axis::PosX,
            Axis::PosY => Axis::NegY,
            Axis::NegY => Axis::PosY,
            Axis::PosZ => Axis::NegZ,
            Axis::NegZ => Axis::PosZ,
        }
    }

    fn from_component(component: usize, positive: bool) -> Axis {
        match (component, positive) {
            (0, true) => Axis::PosX,
            (0, false) => Axis::NegX,
            (1, true) => Axis::PosY,
            (1, false) => Axis::NegY,
            (_, true) => Axis::PosZ,
            (_, false) => Axis::NegZ,
        }
    }

    /// The axis `v` points along, if it is (close to) a coordinate axis.
    pub fn of(v: Vec3) -> Option<Axis> {
        let a = v.abs();
        let k = if a.x >= a.y && a.x >= a.z {
            0
        } else if a.y >= a.z {
            1
        } else {
            2
        };
        (a[k] > 0.9 * v.length()).then(|| Axis::from_component(k, v[k] > 0.))
    }
}

/// How the file is turned: which of its axes points up and which way the
/// character faces. Roblox space puts `up` on +Y and `front` on −Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Orientation {
    pub up: Axis,
    pub front: Axis,
}

impl Orientation {
    /// A file already in Roblox space.
    pub const ROBLOX: Orientation = Orientation {
        up: Axis::PosY,
        front: Axis::NegZ,
    };

    /// `up` and `front` must be perpendicular.
    pub fn new(up: Axis, front: Axis) -> Result<Orientation, Error> {
        if up.component() == front.component() {
            return Err(Error::Orientation);
        }
        Ok(Orientation { up, front })
    }

    /// The rotation from file axes to Roblox axes. Its rows are where Roblox's
    /// X, Y and Z come from: Y is `up`, Z is `−front`, and X = Y × Z keeps it a
    /// proper rotation (no mirroring), which is what puts the character's
    /// left on −X.
    pub fn rotation(&self) -> Mat3 {
        let up = self.up.vector();
        let front = self.front.vector();
        Mat3::from_cols(front.cross(up), up, -front).transpose()
    }

    /// The same up axis with the front turned a quarter turn about it, for a
    /// "rotate 90°" control (`quarter_turns` may be negative).
    pub fn turned(self, quarter_turns: i32) -> Orientation {
        let up = self.up.vector();
        let mut front = self.front.vector();
        for _ in 0..quarter_turns.rem_euclid(4) {
            front = up.cross(front);
        }
        Orientation {
            up: self.up,
            front: Axis::of(front).unwrap_or(self.front),
        }
    }
}

/// Points spread over the surface by area, so a few large triangles count as
/// much as many small ones.
fn surface_samples(mesh: &Mesh) -> Vec<Vec3> {
    let tris: Vec<[Vec3; 3]> = mesh
        .valid_triangles()
        .map(|t| t.map(|i| mesh.positions[i as usize]))
        .collect();
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.;
    let total: f32 = tris.iter().map(area).sum();
    let unit = (total / 12_000.).max(f32::MIN_POSITIVE);
    let mut out = Vec::new();
    for t in &tris {
        let k = ((area(t) / unit).sqrt().ceil() as usize).clamp(1, 40);
        for i in 0..=k {
            for j in 0..=k - i {
                let (u, v) = (i as f32 / k as f32, j as f32 / k as f32);
                out.push(t[0] + (t[1] - t[0]) * u + (t[2] - t[0]) * v);
            }
        }
    }
    out
}

/// Sizes of the 8-connected clusters `points` make on a grid of `cell`.
fn clusters(points: &[Vec2], cell: f32) -> Vec<usize> {
    if points.is_empty() {
        return Vec::new();
    }
    let lo = points
        .iter()
        .fold(Vec2::splat(f32::INFINITY), |a, &p| a.min(p));
    let hi = points
        .iter()
        .fold(Vec2::splat(f32::NEG_INFINITY), |a, &p| a.max(p));
    let w = ((hi.x - lo.x) / cell) as usize + 1;
    let h = ((hi.y - lo.y) / cell) as usize + 1;
    let mut count = vec![0usize; w * h];
    for p in points {
        let x = (((p.x - lo.x) / cell) as usize).min(w - 1);
        let y = (((p.y - lo.y) / cell) as usize).min(h - 1);
        count[y * w + x] += 1;
    }
    let mut seen = vec![false; w * h];
    let mut sizes = Vec::new();
    for start in 0..w * h {
        if count[start] == 0 || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += count[i];
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if count[n] > 0 && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

/// Significant clusters in a slab: tiny specks (a stray vertex, a strand of
/// hair) are not a foot.
fn significant(sizes: &[usize]) -> usize {
    let total: usize = sizes.iter().sum();
    sizes.iter().filter(|&&s| s >= 3 && s * 25 >= total).count()
}

/// Finds the up and front axes from the shape.
///
/// Up is the axis whose bottom slab holds two clusters (the feet) and whose
/// top slab holds one (the head). The thinnest axis is never up: it is the
/// body's depth in every standing pose, while in a T-pose the arm span is as
/// long as the height, so neither the longest axis nor PCA can tell height
/// from span. Front is along the depth axis, towards where the toes stick
/// out past the shins. A format's declared axes break ties.
pub(crate) fn detect(mesh: &Mesh, warnings: &mut Vec<String>) -> Orientation {
    let points = surface_samples(mesh);
    let (lo, hi) = crate::mesh::bounds(points.iter().copied());
    let extent = hi - lo;
    let longest = extent.max_element().max(f32::MIN_POSITIVE);
    let depth = if extent.x <= extent.y && extent.x <= extent.z {
        0
    } else if extent.y <= extent.z {
        1
    } else {
        2
    };

    let mut best: Option<(f32, Axis)> = None;
    for axis in Axis::ALL {
        if axis.component() == depth {
            continue;
        }
        let k = axis.component();
        let sign = axis.vector()[k];
        let (a, b) = ((k + 1) % 3, (k + 2) % 3);
        let along = |p: &Vec3| p[k] * sign;
        let (min, max) = if sign > 0. {
            (lo[k], hi[k])
        } else {
            (-hi[k], -lo[k])
        };
        let slab = 0.05 * (max - min);
        let flat = |p: &Vec3| Vec2::new(p[a], p[b]);
        let bottom: Vec<Vec2> = points
            .iter()
            .filter(|p| along(p) <= min + slab)
            .map(flat)
            .collect();
        let top: Vec<Vec2> = points
            .iter()
            .filter(|p| along(p) >= max - slab)
            .map(flat)
            .collect();
        let cell = 0.025 * longest;
        let spread = |s: &[Vec2]| {
            let (l, h) = s.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(l, h), &p| (l.min(p), h.max(p)),
            );
            ((h - l).max(Vec2::ZERO)).element_product()
        };
        let mut score = 0.;
        if significant(&clusters(&bottom, cell)) == 2 {
            score += 1.;
        }
        if significant(&clusters(&top, cell)) == 1 {
            score += 1.;
        }
        if spread(&bottom) > spread(&top) {
            score += 0.25;
        }
        if mesh.up_hint == Some(axis) {
            score += 0.5;
        }
        // On a full tie the taller axis wins: height beats span except in a
        // T-pose, where the cluster test has already decided.
        score += 0.01 * (max - min) / longest;
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, axis));
        }
    }
    let up = best.map_or(Axis::PosY, |(_, a)| a);
    let k = up.component();
    let sign = up.vector()[k];

    // Depth is the thinner of the two horizontal axes.
    let (h1, h2) = ((k + 1) % 3, (k + 2) % 3);
    let d = if extent[h1] <= extent[h2] { h1 } else { h2 };
    let height = |p: &Vec3| p[k] * sign;
    let (min, max) = if sign > 0. {
        (lo[k], hi[k])
    } else {
        (-hi[k], -lo[k])
    };
    let tall = max - min;
    let mean = |a: f32, b: f32| {
        let (sum, n) = points
            .iter()
            .filter(|p| (min + a * tall..=min + b * tall).contains(&height(p)))
            .fold((0., 0usize), |(s, n), p| (s + p[d], n + 1));
        (n > 0).then(|| sum / n as f32)
    };
    let toes = mean(0., 0.04).zip(mean(0.10, 0.16)).map(|(f, s)| f - s);
    let front = match toes {
        Some(offset) if offset.abs() > 0.01 * tall => Axis::from_component(d, offset > 0.),
        _ => {
            let declared = mesh.front_hint.filter(|f| f.component() != k);
            let front = declared.unwrap_or(Axis::from_component(d, true));
            warnings.push(format!(
                "Couldn\u{2019}t tell the front from the feet; assumed the character faces {front:?}. Turn it if it faces another way."
            ));
            front
        }
    };
    Orientation { up, front }
}
