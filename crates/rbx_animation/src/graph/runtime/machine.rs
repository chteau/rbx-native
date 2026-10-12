//! The state machine: the active state plays, and after each step the
//! transitions leaving it (or leaving "(Any)") that are ready compete.

use crate::graph::model::{WaitFor, When};
use crate::graph::pose::Pose;

use super::{Player, Status};

impl Player {
    /// An Expression transition is ready while its expression is truthy, a
    /// Finished one when the state finishes (or one blend length before,
    /// with BeforeFinished). The highest Priority wins and ties are drawn at
    /// random. UNVERIFIED: an "(Any)" transition never re-enters the state
    /// already playing; the tie-break uses this player's generator, where
    /// Roblox's seed is not documented.
    pub(super) fn state_machine(&mut self, n: usize, dt: f32) -> (Pose, Status) {
        let entry = self.text(n, "EntryState");
        let first = self.pin_index(n, &entry).unwrap_or(0);
        self.begin(n, first);
        let (pose, status) = self.current(n, dt);
        let active = self.states[n].active;
        let count = self.graph.nodes[n].inputs.len();
        let ready: Vec<(u32, usize, f32, crate::easing::Style)> = self.graph.nodes[n]
            .transitions
            .iter()
            .filter(|t| t.to < count && t.to != active && t.from.is_none_or(|f| f == active))
            .filter(|t| match (t.wait_for, t.when) {
                (WaitFor::Trigger, _) => self.condition(&t.expression),
                (WaitFor::Finished, When::Finished) => status.finished,
                (WaitFor::Finished, When::BeforeFinished) => {
                    status.finished || status.remaining <= t.duration
                }
            })
            .map(|t| (t.priority, t.to, t.duration.max(0.), t.curve))
            .collect();
        let Some(best) = ready.iter().map(|t| t.0).max() else {
            return (pose, status);
        };
        let tied: Vec<_> = ready.iter().filter(|t| t.0 == best).collect();
        let pick = if tied.len() > 1 {
            self.rng().below(tied.len())
        } else {
            0
        };
        let (_, to, duration, curve) = *tied[pick];
        self.switch(n, to, (duration, curve));
        (pose, status)
    }
}
