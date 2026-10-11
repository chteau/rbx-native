//! The step's logs: the markers clips pass and the nodes evaluated, each
//! scaled by the nodes above as the step unwinds, which become the events
//! the step reports and each node's live weight.

use super::Player;

/// A clip marker reached this step, as `GetMarkerReachedSignal` reports it,
/// with how much its source counted in the output.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub value: String,
    /// The source's influence under the global event rules (see the
    /// runtime module); never zero here, since a
    /// silenced event is not reported.
    pub weight: f32,
    /// The Clip node it came from.
    pub node: usize,
}

/// Where the event and influence logs stood, so a node can scale what one
/// of its inputs added to them.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    events: usize,
    visits: usize,
}

impl Player {
    /// The markers the last step reached, in the order they were reached.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// How much `node` counted in the last step's output, 0..1: what the
    /// editor shows as each input's "NN%" and fades idle nodes by.
    pub fn weight(&self, node: usize) -> f32 {
        self.weights.get(node).copied().unwrap_or(0.)
    }

    pub(crate) fn mark(&self) -> Mark {
        Mark {
            events: self.emitted.len(),
            visits: self.visits.len(),
        }
    }

    /// Scales what was logged between two marks (one input's evaluation):
    /// its events by `events`, its nodes' influence by `influence`.
    pub(crate) fn weigh(&mut self, from: Mark, to: Mark, events: f32, influence: f32) {
        for e in &mut self.emitted[from.events..to.events] {
            e.weight *= events.max(0.);
        }
        for v in &mut self.visits[from.visits..to.visits] {
            v.1 *= influence.clamp(0., 1.);
        }
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.emitted.push(event);
    }

    /// Turns the logs into what the step reports: the events that kept any
    /// weight, and each node's influence (summed where a node was reached
    /// along more than one path).
    pub(super) fn settle(&mut self) {
        self.events = std::mem::take(&mut self.emitted);
        self.events.retain(|e| e.weight > 0.);
        self.weights = vec![0.; self.graph.nodes.len()];
        for &(node, w) in &self.visits {
            self.weights[node] = (self.weights[node] + w).min(1.);
        }
    }
}
