//! Studio's `Grid3D`, the lattice a pole or a cylinder's side draws.

use glam::{Vec2, Vec3};

use super::styled;
use crate::dragger::{Line, PASSIVE};

/// Studio's `Grid3D`: the lattice lines `at + a·i + b·j` for whole `i` and
/// `j` in the ranges given, leaving out `exclude`; with `radius`, each line
/// clipped to the disc of that radius round `at` (all in lattice steps).
pub(super) struct Lattice {
    pub(super) at: Vec3,
    pub(super) a: Vec3,
    pub(super) b: Vec3,
    pub(super) min: Vec2,
    pub(super) max: Vec2,
    pub(super) radius: Option<f32>,
    pub(super) exclude: (i64, i64),
}

/// The whole steps from `first` to `last`, thinned as Studio's `Grid3D`
/// thins a lattice more than 1024 lines across: every
/// `2^floor(log2(range / 1024))`th. However small the grid against the part,
/// never more than about 2048 of them.
pub(super) fn thinned(first: i64, last: i64) -> impl Iterator<Item = i64> {
    let range = last.saturating_sub(first).max(0);
    let step = if range > 1024 {
        1i64 << (range / 1024).ilog2()
    } else {
        1
    };
    (first..=last).step_by(step as usize)
}

impl Lattice {
    /// Its lines, each drawn at 0.4 transparency depth-tested and 0.85 over
    /// everything.
    pub(super) fn lines(&self) -> Vec<Line> {
        let mut pairs = Vec::new();
        let (a, b) = (self.a, self.b);
        let mut run =
            |min: f32, max: f32, other: (f32, f32), skip: i64, across: Vec3, along: Vec3| {
                let (first, last) = ((min - 0.001).ceil() as i64, (max + 0.001).floor() as i64);
                for i in thinned(first, last) {
                    if i == skip {
                        continue;
                    }
                    let (mut low, mut high) = other;
                    if let Some(radius) = self.radius {
                        let half = (f64::from(radius).powi(2) - (i as f64).powi(2)).sqrt() as f32;
                        if half.is_nan() {
                            continue;
                        }
                        (low, high) = (low.max(-half), high.min(half));
                    }
                    if low < high {
                        let base = self.at + across * i as f32;
                        pairs.push([base + along * low, base + along * high]);
                    }
                }
            };
        run(
            self.min.x,
            self.max.x,
            (self.min.y, self.max.y),
            self.exclude.0,
            a,
            b,
        );
        run(
            self.min.y,
            self.max.y,
            (self.min.x, self.max.x),
            self.exclude.1,
            b,
            a,
        );
        styled(pairs, PASSIVE, 0.6, 0.15)
    }
}
