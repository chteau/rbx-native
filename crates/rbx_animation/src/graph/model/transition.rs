//! A state machine's transitions.

use crate::easing::Style;

/// `Enum.AnimationNodeWaitFor`. On a state-machine transition the editor
/// labels `Trigger` "Expression".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaitFor {
    /// The transition's expression (or trigger) decides.
    #[default]
    Trigger,
    /// The state's animation finishing decides.
    Finished,
}

/// `Enum.AnimationNodeTransitionWhen`: for a transition waiting on
/// [`WaitFor::Finished`], whether the blend starts when the state ends or one
/// blend length before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum When {
    #[default]
    Finished,
    BeforeFinished,
}

/// The engine's length for a transition with no `Duration` attribute.
pub(crate) const ENGINE_TRANSITION_DURATION: f32 = 0.15;

/// A state machine's transition between two of its states (inputs).
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// `TransitionId`, a GUID; empty until the transition is first saved.
    pub id: String,
    /// The input index left; `None` is the "(Any)" state.
    pub from: Option<usize>,
    pub to: usize,
    pub wait_for: WaitFor,
    pub expression: String,
    /// Higher wins when several are ready at once (0–65535); on a tie a
    /// state's own transitions beat "(Any)" ones, then the first listed.
    pub priority: u32,
    /// The editor's "Length", in seconds.
    pub duration: f32,
    pub curve: Style,
    pub when: When,
}

impl Transition {
    /// A transition as the editor draws a new one: Length 0.2, Linear,
    /// Priority 1, waiting on an (empty) expression.
    pub fn new(from: Option<usize>, to: usize) -> Transition {
        Transition {
            id: String::new(),
            from,
            to,
            wait_for: WaitFor::Trigger,
            expression: String::new(),
            priority: 1,
            duration: 0.2,
            curve: Style::Linear,
            when: When::Finished,
        }
    }
}
