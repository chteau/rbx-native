//! What the single-input and layering node kinds do in a step: Clip,
//! Over/Add/Subtract, Mask and OneShot.

use std::collections::BTreeMap;

use crate::easing::Style;
use crate::graph::model::Value;
use crate::graph::pose::{self, Pose, Posed};
use crate::sample::sample_joint;

use super::{curve_at, Player, Status};

mod pick;

pub(super) fn curve_named(value: Option<Value>) -> Style {
    match value {
        Some(Value::Text(t)) if t == "CubicV2" => Style::CubicV2,
        Some(Value::Number(n)) if n as u32 == Style::CubicV2.value() => Style::CubicV2,
        _ => Style::Linear,
    }
}

impl Player {
    /// The part of the clip that plays: [TrimStart, TrimEnd] in absolute
    /// seconds while Trim is on. UNVERIFIED: a TrimEnd at or before TrimStart
    /// (as a node gets before a clip is imported) plays on to the clip's end.
    pub(super) fn clip_span(&self, n: usize) -> (f32, f32) {
        let length = self.clip_of(n).map_or(0., |c| c.length());
        if !self.flag(n, "Trim") {
            return (0., length);
        }
        let start = self.number(n, "TrimStart", 0.).clamp(0., length);
        let end = self.number(n, "TrimEnd", 0.).min(length);
        (start, if end > start { end } else { length })
    }

    /// The clip's rate: Speed, negative when Reverse is on.
    pub(super) fn clip_rate(&self, n: usize) -> f32 {
        let reverse = self.flag(n, "Reverse");
        self.number(n, "Speed", 1.) * if reverse { -1. } else { 1. }
    }

    pub(super) fn clip(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let rate = self.clip_rate(n);
        let mode = self.text(n, "PlayMode");
        let (start, end) = self.clip_span(n);
        let span = end - start;
        let once = mode == "OnceAndHold" || mode == "OnceAndReset";
        let state = self.state(n);
        // `time` runs over 0..span from where playback begins, which is the
        // span's end for a reversed clip.
        if !state.started {
            state.started = true;
            state.time = if rate < 0. { span } else { 0. };
        }
        let direction = if state.backwards { -1. } else { 1. };
        let mut t = if state.done {
            state.time
        } else {
            state.time + dt * rate * direction
        };
        let mut finished = state.done;
        if span > 0. && !state.done {
            match mode.as_str() {
                "PingPong" => {
                    if t > span {
                        t = 2. * span - t;
                        state.backwards = !state.backwards;
                        finished = rate < 0.;
                    } else if t < 0. {
                        t = -t;
                        state.backwards = !state.backwards;
                        finished = rate > 0.;
                    }
                    t = t.clamp(0., span);
                }
                _ if once => {
                    finished = if rate < 0. { t <= 0. } else { t >= span };
                    t = t.clamp(0., span);
                    if finished {
                        state.done = true;
                        // OnceAndReset snaps back to where playback began.
                        if mode == "OnceAndReset" {
                            t = if rate < 0. { span } else { 0. };
                        }
                    }
                }
                _ => {
                    if !(0. ..span).contains(&t) {
                        finished = true;
                        t = t.rem_euclid(span);
                    }
                }
            }
        }
        state.time = t;
        let heading = rate * direction;
        let left = if heading >= 0. { span - t } else { t };
        let remaining = if state.done {
            0.
        } else if mode == "PingPong" {
            // A ping-pong finishes once back where it began, so the way out
            // still has the way back to come.
            left + if state.backwards { 0. } else { span }
        } else {
            left
        };
        let status = Status {
            finished,
            remaining: remaining / rate.abs().max(1e-6),
        };
        let Some(clip) = self.clip_of(n) else {
            return (Pose::new(), status);
        };
        let at = start + t;
        let pose = clip
            .joints()
            .into_iter()
            .filter_map(|j| {
                sample_joint(clip, &j, at).map(|s| {
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
        (
            layered(self.kind(n).type_name(), &base, &top, weight),
            status,
        )
    }

    /// Weights each joint by the shared mask the node names (`1 − w` when
    /// inverted). UNVERIFIED: a joint the mask does not list passes at full
    /// weight; the editor seeds every joint at 100 %, so that is the likely
    /// reading of a missing one.
    pub(super) fn mask(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let invert = self.flag(n, "Invert");
        let name = self.text(n, "Mask");
        let weights: BTreeMap<String, f32> = self
            .graph
            .mask(&name)
            .map(|m| {
                m.flatten()
                    .into_iter()
                    .map(|(j, w)| (j.to_owned(), w))
                    .collect()
            })
            .unwrap_or_default();
        let (mut pose, status) = self.child(n, 0, dt);
        for (joint, posed) in pose.iter_mut() {
            let w = weights.get(joint).copied().unwrap_or(1.).clamp(0., 1.);
            posed.weight *= if invert { 1. - w } else { w };
        }
        (pose, status)
    }

    /// A rising edge on Trigger starts a shot: the OneShot input, restarted,
    /// blends in over the in-transition, plays, and blends out over the
    /// out-transition when it finishes (or one out-length before, with
    /// BeforeFinished), or as soon as Cancel is on. While a shot plays a new
    /// edge restarts it only if Interruptible.
    pub(super) fn one_shot(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let trigger = self.flag(n, "Trigger");
        let cancel = self.flag(n, "Cancel");
        let interruptible = self.prop(n, "Interruptible").is_none_or(|v| v.truthy());
        let fade_in = self.number(n, "TransitionInDuration", 0.15).max(0.);
        let fade_out = self.number(n, "TransitionOutDuration", 0.15).max(0.);
        let curve_in = curve_named(self.prop(n, "TransitionInCurve"));
        let curve_out = curve_named(self.prop(n, "TransitionOutCurve"));
        let mode = self.text(n, "BlendMode");
        let before = self.text(n, "TransitionOutWhen") == "BeforeFinished";

        let (base, status) = self.child(n, 0, dt);
        let state = &self.states[n];
        let rising = trigger && !state.armed;
        if rising && (!state.started || interruptible) {
            // UNVERIFIED: that a shot restarts its subtree from time 0.
            if let Some(child) = self.source(n, 1) {
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
            s.shot_from = s.shot;
            s.time = 0.;
        }
        s.shot = if s.fading_out {
            let t = if fade_out > 0. { s.time / fade_out } else { 1. };
            s.shot_from * (1. - curve_at(curve_out, t))
        } else if fade_in > 0. {
            curve_at(curve_in, s.time / fade_in)
        } else {
            1.
        };
        if s.fading_out && s.shot <= 0. {
            s.shot = 0.;
            s.started = false;
            return (base, status);
        }
        let w = s.shot.clamp(0., 1.);
        (layered(&mode, &base, &shot, w), status)
    }
}

/// Over, Add or Subtract, by the node type or `BlendMode` item naming it.
fn layered(mode: &str, base: &Pose, top: &Pose, weight: f32) -> Pose {
    match mode {
        "Add" | "AddNode" => pose::add(base, top, weight),
        "Subtract" | "SubtractNode" => pose::subtract(base, top, weight),
        _ => pose::over(base, top, weight),
    }
}
