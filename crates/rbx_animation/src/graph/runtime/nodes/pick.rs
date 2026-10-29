//! The nodes that pick one input at a time, Select, PrioritySelect,
//! Sequence and RandomSequence, and the blend they share with the state
//! machine.
//!
//! A transition into input i takes i's `TransitionOverride*` settings where
//! they are set and the node's `Default*` ones where not, except that a
//! BeforeFinished transition (which starts blending one length before the
//! outgoing input ends) takes the outgoing input's.

use crate::easing::Style;
use crate::graph::model::{NodeKind, Value, DEFAULT_TRANSITION_DURATION};
use crate::graph::pose::{self, Pose};

use super::super::rng::Mt19937;
use super::super::{Fade, Player, Status, NONE};
use super::curve_named;

impl Player {
    /// An input's transition setting `name` (`Duration`, `Curve`, `WaitFor`,
    /// `When`, `Interruptible`): its override, else the node's default.
    pub(in crate::graph::runtime) fn setting(
        &self,
        n: usize,
        input: usize,
        name: &str,
    ) -> Option<Value> {
        let default = match name {
            "Duration" | "Curve" | "When" => format!("DefaultTransition{name}"),
            other => format!("Default{other}"),
        };
        self.input_prop(n, input, &format!("TransitionOverride{name}"))
            .or_else(|| self.prop(n, &default))
    }

    fn transition_of(&self, n: usize, input: usize) -> (f32, Style) {
        let duration = self
            .setting(n, input, "Duration")
            .map_or(DEFAULT_TRANSITION_DURATION, |v| v.number()) as f32;
        (
            duration.max(0.),
            curve_named(self.setting(n, input, "Curve")),
        )
    }

    /// Moves a picking node to input `to` (or to [`NONE`]), blending from
    /// the input it was on. A switch during a blend drops the blend's source
    /// at once and blends from its target, as real Studio does: an A→B
    /// blend interrupted halfway by C shows B fading into C, with no A.
    pub(in crate::graph::runtime) fn switch(&mut self, n: usize, to: usize, blend: (f32, Style)) {
        let (duration, curve) = blend;
        if let Some(child) = self.source(n, to) {
            self.restart(child);
        }
        let state = self.state(n);
        let from = state.active;
        state.active = to;
        // From the rest pose the new input cuts in, as real Studio's Select
        // and Priority Select do; only a fade out to rest is blended.
        state.fade = (duration > 0. && from != NONE).then_some(Fade {
            from,
            elapsed: 0.,
            duration,
            curve,
        });
    }

    /// The active input, blended from the previous one while a fade runs.
    /// Only the inputs in play pass events on; during a fade both do, each
    /// scaled by its share of the blend. Real Studio fires every marker of
    /// both sides for the whole transition, the outgoing side's from the
    /// switching step on, however small its share. UNVERIFIED: after a 1 s
    /// blend Studio's incoming markers ran one step early, as if the clip
    /// gained a step as the blend ended; the incoming clock here stays
    /// steady until the probe's `Handover` cases are run.
    pub(in crate::graph::runtime) fn current(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let active = self.states[n].active;
        let start = self.mark();
        let (to, status) = self.child(n, active, dt);
        let Some(mut fade) = self.states[n].fade.take() else {
            return (to, status);
        };
        let middle = self.mark();
        let from = self.child(n, fade.from, dt).0;
        let end = self.mark();
        fade.elapsed += dt;
        let progress = fade.progress();
        self.weigh(start, middle, progress, progress);
        self.weigh(middle, end, 1. - progress, 1. - progress);
        let pose = pose::mix(&from, &to, progress);
        if !fade.done() {
            self.state(n).fade = Some(fade);
        }
        (pose, status)
    }

    pub(in crate::graph::runtime) fn begin(&mut self, n: usize, first: usize) {
        if self.states[n].started {
            return;
        }
        if let Some(child) = self.source(n, first) {
            self.restart(child);
        }
        let state = self.state(n);
        state.started = true;
        state.active = first;
    }

    /// Plays the input whose pin name Selection holds. A Selection naming no
    /// input (the empty default included) plays none: real Studio starts on
    /// the rest pose, and fades the current input out to it over the
    /// default transition when Selection is changed to an unknown name.
    pub(in crate::graph::runtime) fn select(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let wanted = self
            .pin_index(n, &self.text(n, "Selection"))
            .unwrap_or(NONE);
        self.begin(n, wanted);
        if wanted != self.states[n].active {
            let blend = self.transition_of(n, wanted);
            self.switch(n, wanted, blend);
        }
        self.current(n, dt)
    }

    /// Plays the first input, top down, whose Trigger is on, and none (the
    /// rest pose, faded to over the default transition) while every Trigger
    /// is off. As measured in real Studio, the current input's Interruptible
    /// setting (Always, once it has Finished, or while its
    /// InterruptibleTrigger is on) guards only a move to a higher input,
    /// even after its own Trigger turned off; once its Trigger is off, a
    /// move down or to none happens at once.
    pub(in crate::graph::runtime) fn priority_select(
        &mut self,
        n: usize,
        dt: f32,
    ) -> (Pose, Status) {
        let connected = self.connected(n);
        let wanted = connected
            .iter()
            .copied()
            .find(|&i| self.input_prop(n, i, "Trigger").is_some_and(|v| v.truthy()));
        self.begin(n, wanted.unwrap_or(NONE));
        let active = self.states[n].active;
        let rank = |i: usize| connected.iter().position(|&c| c == i).unwrap_or(usize::MAX);
        let to = wanted.unwrap_or(NONE);
        if to != active {
            let rule = self.setting(n, active, "Interruptible").map(|v| v.text());
            let allowed = active == NONE
                || rank(to) > rank(active)
                || match rule {
                    Some(rule) if rule == "Finished" => self.states[n].done,
                    Some(rule) if rule == "Trigger" => self
                        .input_prop(n, active, "InterruptibleTrigger")
                        .is_some_and(|v| v.truthy()),
                    _ => true,
                };
            if allowed {
                let blend = self.transition_of(n, to);
                self.switch(n, to, blend);
            }
        }
        let (pose, status) = self.current(n, dt);
        self.state(n).done = status.finished;
        (pose, status)
    }

    /// Sequence plays its inputs in pin order, RandomSequence picks one by
    /// Weight each time the last completes. An input is left when it
    /// finishes (Wait For Finished, or one blend length early with When
    /// BeforeFinished) or, in a Sequence waiting for Trigger, when its
    /// WaitForTrigger is on. LoopCount / PlayCount 0 runs forever; once the
    /// count is reached the last input's own looping or holding carries on.
    pub(in crate::graph::runtime) fn sequence(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let order = self.connected(n);
        let Some(&first) = order.first() else {
            return (Pose::new(), Status::default());
        };
        let random = self.kind(n) == NodeKind::RandomSequence;
        if !self.states[n].started {
            // Seed −1 is a fresh draw each time the node starts.
            let seed = self.prop(n, "Seed").map_or(-1., |v| v.number());
            let rng = if seed.round() == -1. {
                Mt19937::new(self.rng().next_u32())
            } else {
                Mt19937::from_seed(seed)
            };
            self.state(n).rng = Some(rng);
            let start = if random { self.pick(n, &order) } else { first };
            self.begin(n, start);
        }
        let (pose, status) = self.current(n, dt);
        let held = Status {
            finished: false,
            ..status
        };
        let limit = self.number(n, if random { "PlayCount" } else { "LoopCount" }, 0.) as u32;
        if limit > 0 && self.states[n].count >= limit {
            return (pose, held);
        }
        let active = self.states[n].active;
        let by_trigger = !random
            && self
                .setting(n, active, "WaitFor")
                .is_some_and(|v| v.text() == "Trigger");
        let early = self
            .setting(n, active, "When")
            .is_some_and(|v| v.text() == "BeforeFinished");
        let outgoing = self.transition_of(n, active);
        let advance = if by_trigger {
            self.input_prop(n, active, "WaitForTrigger")
                .is_some_and(|v| v.truthy())
        } else {
            status.finished || (early && status.remaining <= outgoing.0)
        };
        if !advance {
            return (pose, held);
        }
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
            let blend = if early && !by_trigger {
                outgoing
            } else {
                self.transition_of(n, next)
            };
            self.switch(n, next, blend);
        }
        // An endless sequence finishes each cycle, a counted one at its end.
        let finished = if limit == 0 { wrapped } else { done };
        (pose, Status { finished, ..status })
    }

    /// A weighted draw among `order`: one draw `u` in [0, 1), the input
    /// whose share of the summed weights holds `u · total`. The same input
    /// can play twice running, as it does in real Studio.
    fn pick(&mut self, n: usize, order: &[usize]) -> usize {
        let weights: Vec<f64> = order
            .iter()
            .map(|&i| f64::from(self.input_number(n, i, "Weight", 1.)).max(0.))
            .collect();
        let total: f64 = weights.iter().sum();
        let unit = match self.states[n].rng.as_mut() {
            Some(rng) => rng.unit(),
            None => self.rng.unit(),
        };
        let mut roll = unit * total;
        for (&i, w) in order.iter().zip(&weights) {
            if roll < *w {
                return i;
            }
            roll -= w;
        }
        *order.last().unwrap_or(&0)
    }
}
