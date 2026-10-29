//! Runs an Animation Graph: steps it by a time delta with the current
//! parameter values and returns the rig's pose.
//!
//! A node only advances while something above it is using it: an unselected
//! input or an inactive state is paused. Becoming active again restarts it,
//! along with everything under it. Transitions blend over their length along
//! their curve; only Linear and CubicV2 (eased in and out, the editor's
//! "In/Out") are honoured, anything else runs Linear.
//!
//! The semantics follow Roblox's documentation, and where it is silent,
//! what real Studio was measured doing (Blend2D's weights, the layering
//! maths, RandomSequence's generator and seeding, the transition curves, an
//! interrupted blend, how long a trigger lasts, events across a transition,
//! state-machine ties and "(Any)", Priority Select with its Triggers off, a
//! Mask naming no mask, phase sync;
//! `docs/research/animation-graph-measured.md`). What is still a reading
//! rather than a measurement is marked UNVERIFIED on the code that makes
//! it. Transitions are always
//! crossfades: `AnimationNodeTransitionType`'s InertialBlend and DeadBlend
//! exist in the API but nothing sets them. Root motion (`Animator.RootMotion`,
//! hidden and undocumented) is not extracted.
//!
//! Clip markers become [`Event`]s, which climb the graph as Roblox's
//! "global event rules" have them: each carries its source's influence,
//! nodes that pick an input pass only the active one's (both sides, scaled
//! by the blend, while a transition runs), a blend passes only its heaviest
//! input's, and a weight of zero silences an event. The same bookkeeping
//! gives each node the live weight the editor shows (`GetDebugData`).

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};

use crate::clip::Clip;
use crate::easing::{ease, Direction, Style};
use crate::frame::Frame;
use crate::graph::expr;
use crate::graph::model::{Graph, NodeKind, ParamKind, Prop, Value};
use crate::graph::pose::{self, Pose};

mod blend;
mod events;
mod machine;
mod nodes;
mod rng;

pub use events::Event;
use rng::Mt19937;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fade {
    pub from: usize,
    pub elapsed: f32,
    pub duration: f32,
    pub curve: Style,
}

impl Fade {
    pub fn progress(&self) -> f32 {
        if self.duration <= 0. {
            return 1.;
        }
        curve_at(self.curve, self.elapsed / self.duration)
    }

    pub fn done(&self) -> bool {
        self.elapsed >= self.duration
    }
}

/// A transition curve at `t` (clamped to 0..1). CubicV2 is the cubic eased
/// in and out, `4t³` up to the midpoint: real Studio's blend weights over a
/// one-second CubicV2 transition match it at every 1/30 s step.
pub(crate) fn curve_at(curve: Style, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    match curve {
        Style::CubicV2 => ease(Style::CubicV2, Direction::InOut, t),
        _ => t,
    }
}

/// `State::active` of a picking node playing no input, which shows the
/// rest pose: a Select whose Selection names no pin, a PrioritySelect
/// before any Trigger.
pub(crate) const NONE: usize = usize::MAX;

/// What a node keeps between steps.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct State {
    pub time: f32,
    /// The input (or state) in play, for the kinds that pick one.
    pub active: usize,
    pub fade: Option<Fade>,
    /// Loops or plays completed, for Sequence and RandomSequence.
    pub count: u32,
    pub started: bool,
    /// A play-once clip has ended; PrioritySelect: the active input finished
    /// on the last step.
    pub done: bool,
    /// OneShot: the trigger's last value, to see its rising edge.
    pub armed: bool,
    /// OneShot: 0 idle, else the shot's own blend weight.
    pub shot: f32,
    /// OneShot: the weight the blend-out started from.
    pub shot_from: f32,
    pub fading_out: bool,
    pub rng: Option<Mt19937>,
    /// PingPong clips: true while playing backwards.
    pub backwards: bool,
    /// State machine: how the active state's last step ended.
    pub last: Option<Status>,
}

/// How a node's evaluation ended this step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Status {
    /// A play-once animation ended, or a looping one completed a cycle.
    pub finished: bool,
    /// Seconds left before it next finishes.
    pub remaining: f32,
}

pub struct Player {
    graph: Graph,
    clips: Vec<Option<Clip>>,
    states: Vec<State>,
    /// Values `set` since the graph loaded; the rest read their defaults.
    params: BTreeMap<String, Value>,
    /// Trigger parameters the last step saw on, cleared as the next begins.
    consumed: Vec<String>,
    /// Seeds Random Sequences whose `Seed` is -1.
    rng: Mt19937,
    /// This step's events and node evaluations, each with the weight the
    /// nodes above have scaled it by so far.
    emitted: Vec<Event>,
    visits: Vec<(usize, f32)>,
    events: Vec<Event>,
    weights: Vec<f32>,
}

impl Player {
    /// `resolve` finds the clip a Clip node's `AnimationId` names.
    pub fn new(graph: Graph, resolve: &dyn Fn(&str) -> Option<Clip>) -> Player {
        let clips = graph
            .nodes
            .iter()
            .map(|n| match (n.kind, n.props.get("AnimationId")) {
                (NodeKind::Clip, Some(Prop::Value(Value::Text(id)))) => resolve(id),
                _ => None,
            })
            .collect();
        Player {
            states: vec![State::default(); graph.nodes.len()],
            weights: vec![0.; graph.nodes.len()],
            graph,
            clips,
            params: BTreeMap::new(),
            consumed: Vec::new(),
            rng: Mt19937::new(RandomState::new().hash_one(0u8) as u32),
            emitted: Vec::new(),
            visits: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Makes every random choice from here on reproducible.
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = Mt19937::new(seed as u32);
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// `AnimationTrack:SetParameter`. A NaN or infinite number becomes 0, as
    /// Roblox's does.
    pub fn set(&mut self, name: &str, value: Value) {
        if self
            .graph
            .param(name)
            .is_some_and(|p| p.kind == ParamKind::Trigger)
        {
            if value.truthy() {
                self.fire(name);
            }
            return;
        }
        let value = match value {
            Value::Number(n) if !n.is_finite() => Value::Number(0.),
            other => other,
        };
        self.params.insert(name.to_owned(), value);
    }

    /// `AnimationTrack:GetParameter`: nil until the parameter is set.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    /// `GetParameterDefaults`: every parameter's default, as the graph's
    /// attributes hold them.
    pub fn defaults(&self) -> BTreeMap<String, Value> {
        self.graph
            .params
            .iter()
            .map(|p| (p.name.clone(), p.default.clone()))
            .collect()
    }

    /// Turns a trigger parameter on for exactly one evaluation, as real
    /// Studio does: the next step sees it and the one after clears it
    /// before evaluating, so it still reads true between the two, and
    /// firing it again in that gap is lost. Only steps count, a zero-length
    /// one included; frames without a step do not.
    pub fn fire(&mut self, name: &str) {
        self.params.insert(name.to_owned(), Value::Bool(true));
    }

    /// The input a Select, PrioritySelect, Sequence, RandomSequence or state
    /// machine node is currently playing, for the editor to highlight.
    pub fn active(&self, node: usize) -> Option<usize> {
        let n = self.graph.nodes.get(node)?;
        (n.kind.grows_pins() && n.kind != NodeKind::Blend1D && n.kind != NodeKind::Blend2D)
            .then(|| self.states[node].active)
            .filter(|&a| a != NONE)
    }

    /// Restarts everything, as `ResetGraph` does.
    pub fn reset(&mut self) {
        self.states = vec![State::default(); self.graph.nodes.len()];
    }

    /// Advances the graph by `dt` and returns each joint's `Transform`.
    pub fn step(&mut self, dt: f32) -> BTreeMap<String, Frame> {
        self.emitted.clear();
        self.visits.clear();
        for name in std::mem::take(&mut self.consumed) {
            self.params.insert(name, Value::Bool(false));
        }
        self.consumed = self
            .graph
            .params
            .iter()
            .filter(|p| p.kind == ParamKind::Trigger)
            .filter(|p| self.params.get(&p.name).is_some_and(Value::truthy))
            .map(|p| p.name.clone())
            .collect();
        let pose = match self.graph.output() {
            Some(out) => self.eval(out, dt).0,
            None => Pose::new(),
        };
        self.settle();
        pose::resolve(&pose)
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        self.params
            .get(name)
            .cloned()
            .or_else(|| self.graph.param(name).map(|p| p.default.clone()))
    }

    fn read(&self, prop: Option<&Prop>) -> Option<Value> {
        match prop? {
            Prop::Value(v) => Some(v.clone()),
            Prop::Param(name) => self.lookup(name),
            Prop::Expr(source) => expr::eval(source, &|n| self.lookup(n))
                .ok()
                .map(expr::Val::into_value),
        }
    }

    pub(crate) fn prop(&self, node: usize, name: &str) -> Option<Value> {
        self.read(self.graph.nodes[node].props.get(name))
    }

    pub(crate) fn text(&self, node: usize, name: &str) -> String {
        self.prop(node, name).map(|v| v.text()).unwrap_or_default()
    }

    pub(crate) fn flag(&self, node: usize, name: &str) -> bool {
        self.prop(node, name).is_some_and(|v| v.truthy())
    }

    pub(crate) fn number(&self, node: usize, name: &str, default: f64) -> f32 {
        self.prop(node, name).map_or(default, |v| v.number()) as f32
    }

    pub(crate) fn input_prop(&self, node: usize, input: usize, name: &str) -> Option<Value> {
        self.read(self.graph.nodes[node].inputs.get(input)?.props.get(name))
    }

    pub(crate) fn input_number(&self, node: usize, input: usize, name: &str, default: f64) -> f32 {
        self.input_prop(node, input, name)
            .map_or(default, |v| v.number()) as f32
    }

    pub(crate) fn condition(&self, source: &str) -> bool {
        expr::eval(source, &|n| self.lookup(n)).is_ok_and(|v| v.into_value().truthy())
    }

    /// Restarts `node` and everything feeding it.
    pub(crate) fn restart(&mut self, node: usize) {
        let mut stack = vec![node];
        let mut seen = vec![false; self.states.len()];
        while let Some(at) = stack.pop() {
            if std::mem::replace(&mut seen[at], true) {
                continue;
            }
            self.states[at] = State::default();
            stack.extend(self.graph.nodes[at].inputs.iter().filter_map(|i| i.node));
        }
    }

    pub(crate) fn source(&self, node: usize, input: usize) -> Option<usize> {
        self.graph.nodes[node].inputs.get(input)?.node
    }

    /// Evaluates whatever is wired into `node`'s `input`-th pin.
    pub(crate) fn child(&mut self, node: usize, input: usize, dt: f32) -> (Pose, Status) {
        match self.source(node, input) {
            Some(child) => self.eval(child, dt),
            None => (Pose::new(), Status::default()),
        }
    }

    /// The indices of the pins something is wired into.
    pub(crate) fn connected(&self, n: usize) -> Vec<usize> {
        self.graph.nodes[n]
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.node.is_some())
            .map(|(k, _)| k)
            .collect()
    }

    pub(crate) fn pin_index(&self, node: usize, pin: &str) -> Option<usize> {
        self.graph.nodes[node]
            .inputs
            .iter()
            .position(|i| i.pin == pin)
    }

    fn eval(&mut self, node: usize, dt: f32) -> (Pose, Status) {
        self.visits.push((node, 1.));
        match self.graph.nodes[node].kind {
            NodeKind::GraphOutput => self.child(node, 0, dt),
            NodeKind::Clip => self.clip(node, dt),
            NodeKind::Speed => {
                let speed = self.number(node, "Speed", 1.);
                let (pose, status) = self.child(node, 0, dt * speed);
                (pose, scaled(status, speed))
            }
            NodeKind::Over | NodeKind::Add | NodeKind::Subtract => self.layer(node, dt),
            NodeKind::Mask => self.mask(node, dt),
            NodeKind::Blend1D | NodeKind::Blend2D => self.blend(node, dt),
            NodeKind::Select => self.select(node, dt),
            NodeKind::PrioritySelect => self.priority_select(node, dt),
            NodeKind::Sequence | NodeKind::RandomSequence => self.sequence(node, dt),
            NodeKind::OneShot => self.one_shot(node, dt),
            NodeKind::StateMachine => self.state_machine(node, dt),
        }
    }

    pub(crate) fn clip_of(&self, node: usize) -> Option<&Clip> {
        self.clips.get(node)?.as_ref()
    }

    pub(crate) fn state(&mut self, node: usize) -> &mut State {
        &mut self.states[node]
    }

    pub(crate) fn kind(&self, node: usize) -> NodeKind {
        self.graph.nodes[node].kind
    }

    pub(crate) fn rng(&mut self) -> &mut Mt19937 {
        &mut self.rng
    }
}

/// A child's status as seen through a speed multiplier.
fn scaled(status: Status, speed: f32) -> Status {
    Status {
        remaining: if speed.abs() > 1e-6 {
            status.remaining / speed.abs()
        } else {
            f32::INFINITY
        },
        ..status
    }
}

#[cfg(test)]
mod tests;
