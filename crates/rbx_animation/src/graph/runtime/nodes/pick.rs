//! The nodes that pick one input at a time: Select, PrioritySelect,
//! Sequence, RandomSequence and the state machine, and the crossfade they
//! share.

use crate::easing::Style;
use crate::graph::model::{NodeKind, Transition, WaitFor};
use crate::graph::pose::{self, Pose};

use super::super::{Fade, Player, Status};
use super::{curve_named, next_random};

impl Player {
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

    pub(in crate::graph::runtime) fn select(&mut self, n: usize, dt: f32) -> (Pose, Status) {
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

    pub(in crate::graph::runtime) fn sequence(&mut self, n: usize, dt: f32) -> (Pose, Status) {
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

    pub(in crate::graph::runtime) fn state_machine(&mut self, n: usize, dt: f32) -> (Pose, Status) {
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
            .fold(None, |best: Option<&Transition>, t| match best {
                Some(b) if b.priority >= t.priority => Some(b),
                _ => Some(t),
            });
        if let Some(t) = chosen {
            self.switch(n, t.to, Some(t.duration), Some(t.curve));
        }
        (pose, status)
    }
}
