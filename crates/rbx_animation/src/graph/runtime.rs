//! Runs an Animation Graph: steps it by a time delta with the current
//! parameter values and returns the rig's pose.
//!
//! A node only advances while something above it is using it: an unselected
//! input or an inactive state is paused. Becoming active again restarts it,
//! along with everything under it. Transitions crossfade over their duration
//! along their curve (Linear, or CubicV2 eased in and out). Two documented
//! behaviours are approximated here. Blend nodes run their inputs unsynced
//! (no phase matching), and Blend2D weighs its inputs by inverse distance
//! rather than by Roblox's undocumented triangulation. Ties between ready
//! state-machine transitions go to the first one listed, where Roblox picks
//! at random.

use std::collections::BTreeMap;

use crate::clip::Clip;
use crate::easing::{ease, Direction, Style};
use crate::frame::Frame;
use crate::graph::expr;
use crate::graph::model::{Graph, NodeKind, ParamKind, Prop, Value};
use crate::graph::pose::{self, Pose};

mod nodes;

#[derive(Clone, Copy, Debug, PartialEq)]
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
        let t = (self.elapsed / self.duration).clamp(0., 1.);
        match self.curve {
            Style::CubicV2 | Style::Cubic => ease(Style::CubicV2, Direction::InOut, t),
            _ => t,
        }
    }

    pub fn done(&self) -> bool {
        self.elapsed >= self.duration
    }
}

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
    /// OneShot: the trigger's last value, to see its rising edge.
    pub armed: bool,
    /// OneShot: 0 idle, else the shot's own blend weight.
    pub shot: f32,
    pub fading_out: bool,
    pub rng: u64,
    /// PingPong clips: true while playing backwards.
    pub backwards: bool,
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
    params: BTreeMap<String, Value>,
    /// Trigger parameters set since the last step, reset after it.
    fired: Vec<String>,
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
        let params = graph
            .params
            .iter()
            .map(|p| (p.name.clone(), p.default.clone()))
            .collect();
        Player {
            states: vec![State::default(); graph.nodes.len()],
            graph,
            clips,
            params,
            fired: Vec::new(),
        }
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// `AnimationTrack:SetParameter`.
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
        self.params.insert(name.to_owned(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    /// Sets a trigger parameter for exactly one step.
    pub fn fire(&mut self, name: &str) {
        self.params.insert(name.to_owned(), Value::Bool(true));
        self.fired.push(name.to_owned());
    }

    /// The input a Select, PrioritySelect, Sequence, RandomSequence or state
    /// machine node is currently playing, for the editor to highlight.
    pub fn active(&self, node: usize) -> Option<usize> {
        let n = self.graph.nodes.get(node)?;
        n.kind.grows_pins().then(|| self.states[node].active)
    }

    /// Restarts everything, as `ResetGraph` does.
    pub fn reset(&mut self) {
        self.states = vec![State::default(); self.graph.nodes.len()];
    }

    /// Advances the graph by `dt` and returns each joint's `Transform`.
    pub fn step(&mut self, dt: f32) -> BTreeMap<String, Frame> {
        let pose = match self.graph.output() {
            Some(out) => self.eval(out, dt).0,
            None => Pose::new(),
        };
        for name in std::mem::take(&mut self.fired) {
            self.params.insert(name, Value::Bool(false));
        }
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

    pub(crate) fn number(&self, node: usize, name: &str, default: f64) -> f32 {
        self.prop(node, name).map_or(default, |v| v.number()) as f32
    }

    pub(crate) fn input_prop(&self, node: usize, input: usize, name: &str) -> Option<Value> {
        self.read(self.graph.nodes[node].inputs.get(input)?.props.get(name))
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

    /// Evaluates whatever is wired into `node`'s `input`-th pin.
    pub(crate) fn child(&mut self, node: usize, input: usize, dt: f32) -> (Pose, Status) {
        match self.graph.nodes[node]
            .inputs
            .get(input)
            .and_then(|i| i.node)
        {
            Some(child) => self.eval(child, dt),
            None => (Pose::new(), Status::default()),
        }
    }

    pub(crate) fn pin_index(&self, node: usize, pin: &str) -> Option<usize> {
        self.graph.nodes[node]
            .inputs
            .iter()
            .position(|i| i.pin == pin)
    }

    fn eval(&mut self, node: usize, dt: f32) -> (Pose, Status) {
        match self.graph.nodes[node].kind {
            NodeKind::GraphOutput => self.child(node, 0, dt),
            NodeKind::Clip => self.clip(node, dt),
            NodeKind::Speed => {
                let speed = self.number(node, "Speed", 1.);
                self.child(node, 0, dt * speed)
            }
            NodeKind::Over | NodeKind::Add | NodeKind::Subtract => self.layer(node, dt),
            NodeKind::Mask => self.mask(node, dt),
            NodeKind::Blend1D => self.blend1d(node, dt),
            NodeKind::Blend2D => self.blend2d(node, dt),
            NodeKind::Select | NodeKind::PrioritySelect => self.select(node, dt),
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
}

#[cfg(test)]
mod tests;
