//! What each node kind does in a step.

use crate::easing::Style;
use crate::graph::model::{NodeKind, Value, WaitFor};
use crate::graph::pose::{self, Pose, Posed};
use crate::sample::sample_joint;

use super::{Fade, Player, Status};

fn curve_named(value: Option<Value>) -> Style {
    match value {
        Some(Value::Text(t)) if t == "CubicV2" => Style::CubicV2,
        Some(Value::Number(n)) if n as u32 == Style::CubicV2.value() => Style::CubicV2,
        _ => Style::Linear,
    }
}

/// xorshift64*: a reproducible stream from a seed, so a fixed `Seed` plays
/// the same order every time.
fn next_random(state: &mut u64) -> f32 {
    if *state == 0 {
        *state = 0x9E37_79B9_7F4A_7C15;
    }
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
}

impl Player {
    pub(super) fn clip(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let reverse = self.prop(n, "Reverse").is_some_and(|v| v.truthy());
        let speed = self.number(n, "Speed", 1.) * if reverse { -1. } else { 1. };
        let mode = self
            .prop(n, "PlayMode")
            .map(|v| v.text())
            .unwrap_or_default();
        let length = self.clip_of(n).map_or(0., |c| c.length());
        let state = self.state(n);
        if !state.started {
            state.started = true;
            state.time = if speed < 0. { length } else { 0. };
        }
        let direction = if state.backwards { -1. } else { 1. };
        let mut t = state.time + dt * speed * direction;
        let mut finished = false;
        let mut silent = false;
        if length > 0. {
            match mode.as_str() {
                "PingPong" => {
                    if t > length {
                        t = 2. * length - t;
                        state.backwards = !state.backwards;
                    } else if t < 0. {
                        t = -t;
                        state.backwards = !state.backwards;
                        finished = true;
                    }
                    t = t.clamp(0., length);
                }
                "OnceAndHold" | "OnceAndReset" => {
                    finished = if speed < 0. { t <= 0. } else { t >= length };
                    t = t.clamp(0., length);
                    silent = finished && mode == "OnceAndReset";
                }
                _ => {
                    if !(0. ..length).contains(&t) {
                        finished = true;
                        t = t.rem_euclid(length);
                    }
                }
            }
        }
        state.time = t;
        let remaining = if speed * direction >= 0. {
            length - t
        } else {
            t
        };
        let status = Status {
            finished,
            remaining: remaining / speed.abs().max(1e-6),
        };
        let Some(clip) = self.clip_of(n) else {
            return (Pose::new(), status);
        };
        if silent {
            return (Pose::new(), status);
        }
        let pose = clip
            .joints()
            .into_iter()
            .filter_map(|j| {
                sample_joint(clip, &j, t).map(|s| {
                    (
                        j,
                        Posed {
                            frame: s.transform,
                            weight: s.weight,
                        },
                    )
                })
            })
            .collect();
        (pose, status)
    }

    pub(super) fn layer(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let weight = self.number(n, "Weight", 1.);
        let (base, status) = self.child(n, 0, dt);
        let (top, _) = self.child(n, 1, dt);
        let pose = match self.kind(n) {
            NodeKind::Add => pose::add(&base, &top, weight),
            NodeKind::Subtract => pose::subtract(&base, &top, weight),
            _ => pose::over(&base, &top, weight),
        };
        (pose, status)
    }

    pub(super) fn mask(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let invert = self.prop(n, "Invert").is_some_and(|v| v.truthy());
        let (mut pose, status) = self.child(n, 0, dt);
        let listed = self.graph.nodes[n]
            .props
            .keys()
            .any(|k| k.starts_with("Mask:"));
        for (joint, posed) in pose.iter_mut() {
            let w = match self.prop(n, &format!("Mask:{joint}")) {
                Some(v) => v.number() as f32,
                None if listed => 0.,
                None => 1.,
            };
            let w = w.clamp(0., 1.);
            posed.weight *= if invert { 1. - w } else { w };
        }
        (pose, status)
    }

    fn connected(&self, n: usize) -> Vec<usize> {
        self.graph.nodes[n]
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.node.is_some())
            .map(|(k, _)| k)
            .collect()
    }

    pub(super) fn blend1d(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let at = self.number(n, "Position", 0.);
        let mut points: Vec<(usize, f32)> = self
            .connected(n)
            .into_iter()
            .map(|i| {
                (
                    i,
                    self.input_prop(n, i, "Position")
                        .map_or(0., |v| v.number() as f32),
                )
            })
            .collect();
        points.sort_by(|a, b| a.1.total_cmp(&b.1));
        match points.len() {
            0 => (Pose::new(), Status::default()),
            1 => self.child(n, points[0].0, dt),
            len => {
                // The segment around `at`, or the end one it extrapolates.
                let seg = points
                    .windows(2)
                    .position(|w| at <= w[1].1)
                    .unwrap_or(len - 2);
                let ((a, pa), (b, pb)) = (points[seg], points[seg + 1]);
                let t = if (pb - pa).abs() > f32::EPSILON {
                    (at - pa) / (pb - pa)
                } else {
                    0.
                };
                let (low, s0) = self.child(n, a, dt);
                let (high, s1) = self.child(n, b, dt);
                (pose::mix(&low, &high, t), if t < 0.5 { s0 } else { s1 })
            }
        }
    }

    pub(super) fn blend2d(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let (x, y) = (self.number(n, "X", 0.), self.number(n, "Y", 0.));
        let polar = self
            .prop(n, "InputMode")
            .is_some_and(|v| v.text() == "Polar");
        let place = |a: f32, b: f32| {
            if polar {
                (b * a.cos(), b * a.sin())
            } else {
                (a, b)
            }
        };
        let target = place(x, y);
        let mut parts = Vec::new();
        let mut best = (f32::INFINITY, Status::default());
        for i in self.connected(n) {
            let px = self.input_prop(n, i, "X").map_or(0., |v| v.number() as f32);
            let py = self.input_prop(n, i, "Y").map_or(0., |v| v.number() as f32);
            let p = place(px, py);
            let d2 = (p.0 - target.0).powi(2) + (p.1 - target.1).powi(2);
            let (pose, status) = self.child(n, i, dt);
            if d2 < best.0 {
                best = (d2, status);
            }
            parts.push((pose, d2));
        }
        if let Some(exact) = parts.iter().position(|(_, d2)| *d2 < 1e-8) {
            return (parts.swap_remove(exact).0, best.1);
        }
        let weighted: Vec<(Pose, f32)> = parts.into_iter().map(|(p, d2)| (p, 1. / d2)).collect();
        (pose::blend(&weighted), best.1)
    }

    /// Moves a picking node to input `to`, fading from the current one.
    fn switch(&mut self, n: usize, to: usize, duration: Option<f32>, curve: Option<Style>) {
        let default = self.number(n, "DefaultTransitionDuration", 0.15);
        let duration = duration
            .or_else(|| {
                self.input_prop(n, to, "TransitionOverrideDuration")
                    .map(|v| v.number() as f32)
            })
            .unwrap_or(default)
            .max(0.);
        let curve = curve.unwrap_or_else(|| {
            curve_named(
                self.input_prop(n, to, "TransitionOverrideCurve")
                    .or_else(|| self.prop(n, "DefaultTransitionCurve")),
            )
        });
        let from = self.states[n].active;
        if let Some(child) = self.graph.nodes[n].inputs.get(to).and_then(|i| i.node) {
            self.restart(child);
        }
        let state = self.state(n);
        state.active = to;
        state.fade = (duration > 0.).then_some(Fade {
            from,
            elapsed: 0.,
            duration,
            curve,
        });
    }

    /// The active input, crossfaded from the previous one while a fade runs.
    fn current(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let active = self.states[n].active;
        let (to, status) = self.child(n, active, dt);
        let Some(mut fade) = self.states[n].fade else {
            return (to, status);
        };
        let (from, _) = self.child(n, fade.from, dt);
        fade.elapsed += dt;
        let t = fade.progress();
        self.state(n).fade = (!fade.done()).then_some(fade);
        (pose::mix(&from, &to, t), status)
    }

    fn begin(&mut self, n: usize, first: usize) {
        if self.states[n].started {
            return;
        }
        if let Some(child) = self.graph.nodes[n].inputs.get(first).and_then(|i| i.node) {
            self.restart(child);
        }
        let state = self.state(n);
        state.started = true;
        state.active = first;
    }

    pub(super) fn select(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let wanted = if self.kind(n) == NodeKind::Select {
            let selection = self
                .prop(n, "Selection")
                .map(|v| v.text())
                .unwrap_or_default();
            self.pin_index(n, &selection)
        } else {
            self.connected(n)
                .into_iter()
                .find(|&i| self.input_prop(n, i, "Trigger").is_some_and(|v| v.truthy()))
        };
        let first = wanted
            .or_else(|| self.connected(n).first().copied())
            .unwrap_or(0);
        self.begin(n, first);
        if let Some(to) = wanted.filter(|&to| to != self.states[n].active) {
            self.switch(n, to, None, None);
        }
        self.current(n, dt)
    }

    pub(super) fn sequence(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let order = self.connected(n);
        let Some(&first) = order.first() else {
            return (Pose::new(), Status::default());
        };
        let random = self.kind(n) == NodeKind::RandomSequence;
        if !self.states[n].started {
            let seed = self.number(n, "Seed", -1.);
            self.state(n).rng = if seed < 0. {
                n as u64 + 1
            } else {
                seed as u64 + 1
            };
            let start = if random { self.pick(n, &order) } else { first };
            self.begin(n, start);
        }
        let (pose, status) = self.current(n, dt);
        let active = self.states[n].active;
        let by_trigger = self
            .prop(n, "DefaultWaitFor")
            .is_some_and(|v| v.text() == "Trigger")
            || self
                .input_prop(n, active, "TransitionOverrideWaitFor")
                .is_some_and(|v| v.text() == "Trigger");
        let advance = if by_trigger {
            self.input_prop(n, active, "WaitForTrigger")
                .is_some_and(|v| v.truthy())
        } else {
            status.finished
        };
        if !advance {
            return (
                pose,
                Status {
                    finished: false,
                    ..status
                },
            );
        }
        let limit = self.number(n, if random { "PlayCount" } else { "LoopCount" }, 0.) as u32;
        let at = order.iter().position(|&i| i == active).unwrap_or(0);
        let (next, wrapped) = if random {
            self.state(n).count += 1;
            (self.pick(n, &order), true)
        } else if at + 1 < order.len() {
            (order[at + 1], false)
        } else {
            self.state(n).count += 1;
            (first, true)
        };
        let done = limit > 0 && self.states[n].count >= limit;
        if !done {
            self.switch(n, next, None, None);
        }
        (
            pose,
            Status {
                finished: wrapped,
                ..status
            },
        )
    }

    fn pick(&mut self, n: usize, order: &[usize]) -> usize {
        let weights: Vec<f32> = order
            .iter()
            .map(|&i| {
                self.input_prop(n, i, "Weight")
                    .map_or(1., |v| v.number() as f32)
                    .max(0.)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        let mut roll = next_random(&mut self.states[n].rng) * total;
        for (&i, w) in order.iter().zip(&weights) {
            if roll < *w {
                return i;
            }
            roll -= w;
        }
        *order.last().unwrap_or(&0)
    }

    pub(super) fn one_shot(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let trigger = self.prop(n, "Trigger").is_some_and(|v| v.truthy());
        let cancel = self.prop(n, "Cancel").is_some_and(|v| v.truthy());
        let interruptible = self.prop(n, "Interruptible").is_none_or(|v| v.truthy());
        let fade_in = self.number(n, "TransitionInDuration", 0.15).max(0.);
        let fade_out = self.number(n, "TransitionOutDuration", 0.15).max(0.);
        let mode = self
            .prop(n, "BlendMode")
            .map(|v| v.text())
            .unwrap_or_default();
        let before = self
            .prop(n, "TransitionOutWhen")
            .is_some_and(|v| v.text() == "BeforeFinished");

        let (base, status) = self.child(n, 0, dt);
        let state = &self.states[n];
        let rising = trigger && !state.armed;
        let idle = state.shot <= 0. && !state.started;
        if rising && (idle || interruptible) {
            if let Some(child) = self.graph.nodes[n].inputs.get(1).and_then(|i| i.node) {
                self.restart(child);
            }
            let s = self.state(n);
            s.started = true;
            s.fading_out = false;
            s.time = 0.;
        }
        self.state(n).armed = trigger;
        if !self.states[n].started {
            return (base, status);
        }
        let (shot, shot_status) = self.child(n, 1, dt);
        let s = self.state(n);
        s.time += dt;
        let ending = shot_status.finished || (before && shot_status.remaining <= fade_out);
        if !s.fading_out && (cancel || ending) {
            s.fading_out = true;
            s.time = 0.;
        }
        s.shot = if s.fading_out {
            if fade_out > 0. {
                1. - s.time / fade_out
            } else {
                0.
            }
        } else if fade_in > 0. {
            (s.time / fade_in).min(1.)
        } else {
            1.
        };
        if s.fading_out && s.shot <= 0. {
            s.shot = 0.;
            s.started = false;
            return (base, status);
        }
        let w = s.shot.clamp(0., 1.);
        let pose = match mode.as_str() {
            "Add" => pose::add(&base, &shot, w),
            "Subtract" => pose::subtract(&base, &shot, w),
            _ => pose::over(&base, &shot, w),
        };
        (pose, status)
    }

    pub(super) fn state_machine(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let entry = self
            .prop(n, "EntryState")
            .map(|v| v.text())
            .unwrap_or_default();
        let first = self.pin_index(n, &entry).unwrap_or(0);
        self.begin(n, first);
        let (pose, status) = self.current(n, dt);
        let active = self.states[n].active;
        let count = self.graph.nodes[n].inputs.len();
        let transitions = self.graph.nodes[n].transitions.clone();
        let chosen = transitions
            .iter()
            .filter(|t| t.to < count && t.to != active && t.from.is_none_or(|f| f == active))
            .filter(|t| match t.wait_for {
                WaitFor::Finished => status.finished,
                WaitFor::Trigger => self.condition(&t.expression),
            })
            .fold(
                None,
                |best: Option<&crate::graph::model::Transition>, t| match best {
                    Some(b) if b.priority >= t.priority => Some(b),
                    _ => Some(t),
                },
            );
        if let Some(t) = chosen {
            self.switch(n, t.to, Some(t.duration), Some(t.curve));
        }
        (pose, status)
    }
}
