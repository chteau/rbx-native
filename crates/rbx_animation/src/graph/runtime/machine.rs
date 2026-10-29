//! The state machine: the active state plays, and after each step the
//! transitions leaving it (or leaving "(Any)") that are ready compete.

use crate::graph::model::{WaitFor, When};
use crate::graph::pose::Pose;

use super::{Player, Status};

impl Player {
    /// An Expression transition is ready while its expression is truthy, a
    /// Finished one when the state finishes (or one blend length before,
    /// with BeforeFinished). At most one is taken per step. As measured in
    /// real Studio: the highest Priority wins; on a tie the state's own
    /// transitions beat "(Any)" ones, then the first listed wins (no
    /// randomness); and an "(Any)" transition never re-enters the state
    /// already playing, so holding its condition on does not restart it.
    pub(super) fn state_machine(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let entry = self.text(n, "EntryState");
        let first = self.pin_index(n, &entry).unwrap_or(0);
        self.begin(n, first);
        // Transitions are taken as the step begins, so the new state plays
        // in the same step, as in real Studio; Finished ones read how the
        // last step ended.
        let last = self.states[n].last.unwrap_or(Status {
            finished: false,
            remaining: f32::INFINITY,
        });
        let active = self.states[n].active;
        let count = self.graph.nodes[n].inputs.len();
        let ready: Vec<(u32, bool, usize, f32, crate::easing::Style)> = self.graph.nodes[n]
            .transitions
            .iter()
            .filter(|t| t.to < count && t.to != active && t.from.is_none_or(|f| f == active))
            .filter(|t| match (t.wait_for, t.when) {
                (WaitFor::Trigger, _) => self.condition(&t.expression),
                (WaitFor::Finished, When::Finished) => last.finished,
                (WaitFor::Finished, When::BeforeFinished) => {
                    last.finished || last.remaining <= t.duration
                }
            })
            .map(|t| {
                (
                    t.priority,
                    t.from.is_some(),
                    t.to,
                    t.duration.max(0.),
                    t.curve,
                )
            })
            .collect();
        // `max_by_key` keeps the last of equals, so walk the list backwards.
        if let Some(&(_, _, to, duration, curve)) = ready.iter().rev().max_by_key(|t| (t.0, t.1)) {
            self.switch(n, to, (duration, curve));
        }
        let (pose, status) = self.current(n, dt);
        self.state(n).last = Some(status);
        (pose, status)
    }
}
