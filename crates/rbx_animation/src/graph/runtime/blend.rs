//! Blend1D and Blend2D: how much each input counts, and phase sync, which
//! needs each subtree's cycle length before it is stepped.

use std::f32::consts::PI;

use crate::graph::model::NodeKind;
use crate::graph::pose::{self, Pose};

use super::{Player, Status};

/// How much more an angle counts than a magnitude in Polar mode, where
/// Roblox says only that "direction is weighted more heavily". UNVERIFIED.
const ANGLE_WEIGHT: f32 = 2.;

impl Player {
    /// Each wired input with its weight. Blend1D interpolates linearly
    /// between the two inputs either side of Position, ordered by their
    /// Position, and extrapolates from the end pair outside them (so one
    /// weight can be negative).
    pub(super) fn blend_weights(&self, n: usize) -> Vec<(usize, f32)> {
        let connected = self.connected(n);
        if self.kind(n) == NodeKind::Blend2D {
            return self.weights_2d(n, &connected);
        }
        let at = self.number(n, "Position", 0.);
        let mut points: Vec<(usize, f32)> = connected
            .iter()
            .map(|&i| (i, self.input_number(n, i, "Position", 0.)))
            .collect();
        points.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut out: Vec<(usize, f32)> = points.iter().map(|&(i, _)| (i, 0.)).collect();
        match points.len() {
            0 => {}
            1 => out[0].1 = 1.,
            len => {
                let seg = points
                    .windows(2)
                    .position(|w| at <= w[1].1)
                    .unwrap_or(len - 2);
                let (pa, pb) = (points[seg].1, points[seg + 1].1);
                let t = if (pb - pa).abs() > f32::EPSILON {
                    (at - pa) / (pb - pa)
                } else {
                    0.
                };
                out[seg].1 = 1. - t;
                out[seg + 1].1 = t;
            }
        }
        out
    }

    /// Gradient-band interpolation: each input's weight is the least, over
    /// every other input, of how far the target still is from passing that
    /// other one, then normalised. Cartesian mode works on (X, Y); Polar
    /// mode on (angle X in radians, magnitude Y), comparing angles and
    /// relative magnitudes. Roblox documents only that Polar weights direction
    /// more and that events come from "the highest-weight active input"
    /// (`AnimationNodeType.yaml`); the editor's code in Client Tracker
    /// (`AnimationEditor` `Blend2DMap`) only draws the pins.
    /// UNVERIFIED: Roblox documents no algorithm for
    /// Blend2D; this is the scheme Unity's freeform blend trees use, chosen
    /// because Roblox's editor draws no triangulation.
    fn weights_2d(&self, n: usize, inputs: &[usize]) -> Vec<(usize, f32)> {
        let polar = self.text(n, "InputMode") == "Polar";
        let target = (self.number(n, "X", 0.), self.number(n, "Y", 0.));
        let points: Vec<(f32, f32)> = inputs
            .iter()
            .map(|&i| {
                (
                    self.input_number(n, i, "X", 0.),
                    self.input_number(n, i, "Y", 0.),
                )
            })
            .collect();
        let band = |i: usize, j: usize| -> Option<f32> {
            let (a, b) = (points[i], points[j]);
            let (ij, ip) = if polar {
                polar_pair(a, b, target)
            } else {
                ((b.0 - a.0, b.1 - a.1), (target.0 - a.0, target.1 - a.1))
            };
            let len2 = ij.0 * ij.0 + ij.1 * ij.1;
            (len2 > 1e-12).then(|| (1. - (ip.0 * ij.0 + ip.1 * ij.1) / len2).clamp(0., 1.))
        };
        let mut weights: Vec<f32> = (0..points.len())
            .map(|i| {
                (0..points.len())
                    .filter(|&j| j != i)
                    .filter_map(|j| band(i, j))
                    .fold(1., f32::min)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        if total > 1e-6 {
            weights.iter_mut().for_each(|w| *w /= total);
        } else if let Some(nearest) = (0..points.len()).min_by(|&a, &b| {
            let d = |p: (f32, f32)| (p.0 - target.0).powi(2) + (p.1 - target.1).powi(2);
            d(points[a]).total_cmp(&d(points[b]))
        }) {
            weights[nearest] = 1.;
        }
        inputs.iter().copied().zip(weights).collect()
    }

    /// One cycle of `node`'s output, in seconds at its current rate; 0 when
    /// it has none (no clip, a paused one).
    pub(super) fn cycle(&self, node: usize) -> f32 {
        let of = |input: usize| self.source(node, input).map_or(0., |c| self.cycle(c));
        match self.kind(node) {
            NodeKind::Clip => {
                let (start, end) = self.clip_span(node);
                let rate = self.clip_rate(node).abs();
                if rate > 1e-6 {
                    (end - start) / rate
                } else {
                    0.
                }
            }
            NodeKind::Speed => {
                let speed = self.number(node, "Speed", 1.).abs();
                if speed > 1e-6 {
                    of(0) / speed
                } else {
                    0.
                }
            }
            NodeKind::Blend1D | NodeKind::Blend2D => self
                .blend_weights(node)
                .into_iter()
                .map(|(i, w)| w.clamp(0., 1.) * of(i))
                .sum(),
            NodeKind::Select
            | NodeKind::PrioritySelect
            | NodeKind::Sequence
            | NodeKind::RandomSequence
            | NodeKind::StateMachine => of(self.states[node].active),
            _ => of(0),
        }
    }

    /// With PhaseSync Synced (the default) every input advances the same
    /// fraction of its cycle per step: the node's virtual duration is the
    /// weighted mean of its inputs' cycles, `D = Σ wᵢ·Dᵢ`, and input i is
    /// stepped by `dt·Dᵢ/D`. Synced inputs all run, so they stay in phase
    /// while weightless; Unsynced ones run at their own rate, and only while
    /// they count. UNVERIFIED beyond Roblox's description of the virtual
    /// duration.
    pub(super) fn blend(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let weights = self.blend_weights(n);
        let synced = self.text(n, "PhaseSync") != "Unsynced";
        let cycles: Vec<f32> = weights
            .iter()
            .map(|&(i, _)| self.source(n, i).map_or(0., |c| self.cycle(c)))
            .collect();
        let virtual_duration: f32 = weights
            .iter()
            .zip(&cycles)
            .map(|(&(_, w), d)| w.clamp(0., 1.) * d)
            .sum();
        let mut parts = Vec::new();
        let mut lead = (f32::NEG_INFINITY, Status::default());
        let mut logged = Vec::new();
        for (&(i, w), &d) in weights.iter().zip(&cycles) {
            if !synced && w == 0. {
                continue;
            }
            let step = if synced && virtual_duration > 1e-6 && d > 1e-6 {
                dt * d / virtual_duration
            } else {
                dt
            };
            let from = self.mark();
            let (pose, status) = self.child(n, i, step);
            logged.push((from, self.mark(), w));
            // The heaviest input speaks for the node, as its events do.
            if w > lead.0 {
                lead = (w, status);
            }
            if w != 0. {
                parts.push((pose, w));
            }
        }
        // Only the heaviest input's events go on, scaled by its weight.
        let heaviest = logged
            .iter()
            .enumerate()
            .max_by(|a, b| a.1 .2.total_cmp(&b.1 .2))
            .map(|(k, _)| k);
        for (k, &(from, to, w)) in logged.iter().enumerate() {
            let events = if Some(k) == heaviest { w.min(1.) } else { 0. };
            self.weigh(from, to, events, w);
        }
        let pose = match parts.len() {
            0 => Pose::new(),
            1 if (parts[0].1 - 1.).abs() < 1e-6 => parts.swap_remove(0).0,
            // Two inputs (Blend1D, possibly extrapolating) mix directly.
            2 if self.kind(n) == NodeKind::Blend1D => {
                pose::mix(&parts[0].0, &parts[1].0, parts[1].1)
            }
            _ => pose::blend(&parts),
        };
        (pose, lead.1)
    }
}

/// Polar gradient-band vectors for the pair (a, b) and the target, each
/// point as (angle, magnitude). Magnitudes are taken relative to the pair's
/// mean; a point at the origin has no direction, so it takes its partner's.
fn polar_pair(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> ((f32, f32), (f32, f32)) {
    let (mut aa, ma) = (a.0, a.1.abs());
    let (mut ab, mb) = (b.0, b.1.abs());
    let (mut ap, mp) = (p.0, p.1.abs());
    if ma < 1e-6 {
        aa = ab;
    }
    if mb < 1e-6 {
        ab = aa;
    }
    if mp < 1e-6 {
        ap = aa;
    }
    let mean = ((ma + mb) * 0.5).max(1e-6);
    let ij = ((mb - ma) / mean, wrap(ab - aa) * ANGLE_WEIGHT);
    let ip = ((mp - ma) / mean, wrap(ap - aa) * ANGLE_WEIGHT);
    (ij, ip)
}

/// An angle difference folded into (−π, π].
fn wrap(angle: f32) -> f32 {
    let a = (angle + PI).rem_euclid(2. * PI) - PI;
    if a <= -PI {
        a + 2. * PI
    } else {
        a
    }
}
