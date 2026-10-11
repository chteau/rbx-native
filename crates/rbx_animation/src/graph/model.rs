//! The authored half of an Animation Graph: nodes, the pins wiring them,
//! their properties, state-machine transitions, and the graph's parameters.

use std::collections::BTreeMap;

use crate::easing::Style;

/// `Enum.AnimationNodeType`, values as Roblox numbers them (15 is unused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeKind {
    Add,
    Over,
    Blend1D,
    Blend2D,
    Clip,
    GraphOutput,
    Mask,
    PrioritySelect,
    RandomSequence,
    Select,
    Sequence,
    Speed,
    Subtract,
    OneShot,
    StateMachine,
}

impl NodeKind {
    /// Every kind a builder can insert (the output node always exists).
    pub const INSERTABLE: [NodeKind; 14] = [
        NodeKind::Clip,
        NodeKind::StateMachine,
        NodeKind::Blend1D,
        NodeKind::Blend2D,
        NodeKind::Select,
        NodeKind::PrioritySelect,
        NodeKind::Sequence,
        NodeKind::RandomSequence,
        NodeKind::OneShot,
        NodeKind::Over,
        NodeKind::Add,
        NodeKind::Subtract,
        NodeKind::Mask,
        NodeKind::Speed,
    ];

    pub fn value(self) -> u32 {
        match self {
            NodeKind::Add => 1,
            NodeKind::Over => 2,
            NodeKind::Blend1D => 3,
            NodeKind::Blend2D => 4,
            NodeKind::Clip => 5,
            NodeKind::GraphOutput => 6,
            NodeKind::Mask => 7,
            NodeKind::PrioritySelect => 8,
            NodeKind::RandomSequence => 9,
            NodeKind::Select => 10,
            NodeKind::Sequence => 11,
            NodeKind::Speed => 12,
            NodeKind::Subtract => 13,
            NodeKind::OneShot => 14,
            NodeKind::StateMachine => 16,
        }
    }

    pub fn from_value(value: u32) -> Option<NodeKind> {
        NodeKind::INSERTABLE
            .into_iter()
            .chain([NodeKind::GraphOutput])
            .find(|k| k.value() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Add => "Add",
            NodeKind::Over => "Over",
            NodeKind::Blend1D => "Blend 1D",
            NodeKind::Blend2D => "Blend 2D",
            NodeKind::Clip => "Clip",
            NodeKind::GraphOutput => "Output",
            NodeKind::Mask => "Mask",
            NodeKind::PrioritySelect => "Priority Select",
            NodeKind::RandomSequence => "Random Sequence",
            NodeKind::Select => "Select",
            NodeKind::Sequence => "Sequence",
            NodeKind::Speed => "Speed",
            NodeKind::Subtract => "Subtract",
            NodeKind::OneShot => "One Shot",
            NodeKind::StateMachine => "State Machine",
        }
    }

    /// The pins a new node starts with. The kinds that take a list of inputs
    /// start with two and can be given more.
    pub fn default_pins(self) -> &'static [&'static str] {
        match self {
            NodeKind::Clip => &[],
            NodeKind::GraphOutput | NodeKind::Speed | NodeKind::Mask => &["Pose"],
            NodeKind::Over => &["Base", "Over"],
            NodeKind::Add => &["Base", "Additive"],
            NodeKind::Subtract => &["A", "B"],
            NodeKind::OneShot => &["Base", "OneShot"],
            NodeKind::StateMachine => &["State1", "State2"],
            NodeKind::Blend1D
            | NodeKind::Blend2D
            | NodeKind::Select
            | NodeKind::PrioritySelect
            | NodeKind::Sequence
            | NodeKind::RandomSequence => &["Input1", "Input2"],
        }
    }

    pub fn grows_pins(self) -> bool {
        matches!(
            self,
            NodeKind::StateMachine
                | NodeKind::Blend1D
                | NodeKind::Blend2D
                | NodeKind::Select
                | NodeKind::PrioritySelect
                | NodeKind::Sequence
                | NodeKind::RandomSequence
        )
    }

    /// The node's own properties and their defaults, from the
    /// `AnimationNodeType` reference.
    pub fn default_props(self) -> Vec<(&'static str, Value)> {
        use Value::{Bool, Number, Text};
        let fade = || ("DefaultTransitionDuration", Number(0.15));
        match self {
            NodeKind::Clip => vec![
                ("AnimationId", Text(String::new())),
                ("PlayMode", Text("Loop".into())),
                ("Speed", Number(1.)),
                ("Reverse", Bool(false)),
            ],
            NodeKind::Speed => vec![("Speed", Number(1.))],
            NodeKind::Over | NodeKind::Add | NodeKind::Subtract => vec![("Weight", Number(1.))],
            NodeKind::Mask => vec![("Invert", Bool(false))],
            NodeKind::Blend1D => vec![("Position", Number(0.))],
            NodeKind::Blend2D => vec![("X", Number(0.)), ("Y", Number(0.))],
            NodeKind::Select => vec![("Selection", Text("Input1".into())), fade()],
            NodeKind::PrioritySelect => vec![fade()],
            NodeKind::Sequence => vec![("LoopCount", Number(0.)), fade()],
            NodeKind::RandomSequence => {
                vec![("Seed", Number(-1.)), ("PlayCount", Number(0.)), fade()]
            }
            NodeKind::OneShot => vec![
                ("Trigger", Bool(false)),
                ("Cancel", Bool(false)),
                ("BlendMode", Text("Over".into())),
                ("TransitionInDuration", Number(0.15)),
                ("TransitionOutDuration", Number(0.15)),
            ],
            NodeKind::StateMachine => vec![("EntryState", Text("State1".into()))],
            NodeKind::GraphOutput => vec![],
        }
    }

    /// Settings each input pin of this kind carries.
    pub fn default_input_props(self) -> Vec<(&'static str, Value)> {
        match self {
            NodeKind::Blend1D => vec![("Position", Value::Number(0.))],
            NodeKind::Blend2D => vec![("X", Value::Number(0.)), ("Y", Value::Number(0.))],
            NodeKind::PrioritySelect => vec![("Trigger", Value::Bool(false))],
            NodeKind::RandomSequence => vec![("Weight", Value::Number(1.))],
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl Value {
    pub fn number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Text(t) => t.trim().parse().unwrap_or(0.),
        }
    }

    /// Luau truthiness: everything but `false` (and nil, which has no value
    /// here) is true.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Bool(false))
    }

    pub fn text(&self) -> String {
        match self {
            Value::Number(n) => format!("{n}"),
            Value::Bool(b) => b.to_string(),
            Value::Text(t) => t.clone(),
        }
    }
}

/// A node property: a fixed value, driven by a graph parameter, or computed
/// by an expression over the parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Value(Value),
    Param(String),
    Expr(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamKind {
    #[default]
    Number,
    Bool,
    Text,
    /// A bool that resets itself one step after it is set.
    Trigger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
    pub default: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub pin: String,
    /// Index into [`Graph::nodes`].
    pub node: Option<usize>,
    pub props: BTreeMap<String, Prop>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaitFor {
    /// The transition's expression (or trigger) decides.
    #[default]
    Trigger,
    /// The state's animation finishing decides.
    Finished,
}

/// A state machine's transition between two of its states (inputs).
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// The input index left; `None` is the "(Any)" state.
    pub from: Option<usize>,
    pub to: usize,
    pub wait_for: WaitFor,
    pub expression: String,
    /// Higher wins when several are ready at once.
    pub priority: u32,
    pub duration: f32,
    pub curve: Style,
}

impl Transition {
    pub fn new(from: Option<usize>, to: usize) -> Transition {
        Transition {
            from,
            to,
            wait_for: WaitFor::Trigger,
            expression: "false".into(),
            priority: 0,
            duration: 0.15,
            curve: Style::Linear,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// `NodeId`: stable across edits, unique within the graph.
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub props: BTreeMap<String, Prop>,
    pub inputs: Vec<Input>,
    pub transitions: Vec<Transition>,
    /// Where the node editor draws it.
    pub position: (f32, f32),
}

impl Node {
    pub fn new(kind: NodeKind, id: &str) -> Node {
        Node {
            id: id.to_owned(),
            name: kind.label().to_owned(),
            kind,
            props: kind
                .default_props()
                .into_iter()
                .map(|(k, v)| (k.to_owned(), Prop::Value(v)))
                .collect(),
            inputs: kind
                .default_pins()
                .iter()
                .map(|pin| Input {
                    pin: (*pin).to_owned(),
                    node: None,
                    props: kind
                        .default_input_props()
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), Prop::Value(v)))
                        .collect(),
                })
                .collect(),
            transitions: Vec::new(),
            position: (0., 0.),
        }
    }

    pub fn input(&self, pin: &str) -> Option<usize> {
        self.inputs.iter().find(|i| i.pin == pin)?.node
    }

    /// Adds the next numbered pin (`Input3`, `State3`, …).
    pub fn add_pin(&mut self) -> Option<&str> {
        if !self.kind.grows_pins() {
            return None;
        }
        let stem = if self.kind == NodeKind::StateMachine {
            "State"
        } else {
            "Input"
        };
        let n = (1..).find(|n| !self.inputs.iter().any(|i| i.pin == format!("{stem}{n}")))?;
        self.inputs.push(Input {
            pin: format!("{stem}{n}"),
            node: None,
            props: self
                .kind
                .default_input_props()
                .into_iter()
                .map(|(k, v)| (k.to_owned(), Prop::Value(v)))
                .collect(),
        });
        self.inputs.last().map(|i| i.pin.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub name: String,
    pub nodes: Vec<Node>,
    pub params: Vec<Param>,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            name: "AnimationGraph".into(),
            nodes: vec![Node::new(NodeKind::GraphOutput, "Output")],
            params: Vec::new(),
        }
    }
}

impl Graph {
    pub fn output(&self) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.kind == NodeKind::GraphOutput)
    }

    /// Adds a node with a fresh id and returns its index.
    pub fn add(&mut self, kind: NodeKind) -> usize {
        let n = (1..)
            .find(|n| {
                !self
                    .nodes
                    .iter()
                    .any(|node| node.id == format!("{}{n}", kind.label().replace(' ', "")))
            })
            .unwrap_or(0);
        let id = format!("{}{n}", kind.label().replace(' ', ""));
        self.nodes.push(Node::new(kind, &id));
        self.nodes.len() - 1
    }

    /// Wires `from`'s output into `to`'s `pin`. Refused when it would make a
    /// cycle, which no node could evaluate.
    pub fn connect(&mut self, from: usize, to: usize, pin: &str) -> bool {
        if from >= self.nodes.len() || to >= self.nodes.len() || self.reaches(from, to) {
            return false;
        }
        let Some(input) = self.nodes[to].inputs.iter_mut().find(|i| i.pin == pin) else {
            return false;
        };
        input.node = Some(from);
        true
    }

    pub fn disconnect(&mut self, to: usize, pin: &str) {
        if let Some(input) = self
            .nodes
            .get_mut(to)
            .and_then(|n| n.inputs.iter_mut().find(|i| i.pin == pin))
        {
            input.node = None;
        }
    }

    /// Whether `to` feeds (directly or not) into `from`, or is `from`.
    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut stack = vec![from];
        let mut seen = vec![false; self.nodes.len()];
        while let Some(at) = stack.pop() {
            if at == to {
                return true;
            }
            if std::mem::replace(&mut seen[at], true) {
                continue;
            }
            stack.extend(self.nodes[at].inputs.iter().filter_map(|i| i.node));
        }
        false
    }

    /// Removes a node, unwiring everything that pointed at it. The output
    /// node stays.
    pub fn remove(&mut self, index: usize) -> bool {
        if self
            .nodes
            .get(index)
            .is_none_or(|n| n.kind == NodeKind::GraphOutput)
        {
            return false;
        }
        self.nodes.remove(index);
        for node in &mut self.nodes {
            for input in &mut node.inputs {
                input.node = match input.node {
                    Some(i) if i == index => None,
                    Some(i) if i > index => Some(i - 1),
                    other => other,
                };
            }
        }
        true
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_values_match_the_enum() {
        assert_eq!(NodeKind::StateMachine.value(), 16);
        assert_eq!(NodeKind::from_value(15), None);
        for kind in NodeKind::INSERTABLE {
            assert_eq!(NodeKind::from_value(kind.value()), Some(kind));
        }
    }

    #[test]
    fn connect_refuses_cycles_and_remove_unwires() {
        let mut g = Graph::default();
        let speed = g.add(NodeKind::Speed);
        let clip = g.add(NodeKind::Clip);
        let out = g.output().unwrap();
        assert!(g.connect(speed, out, "Pose"));
        assert!(g.connect(clip, speed, "Pose"));
        assert!(!g.connect(out, speed, "Pose"), "would loop");
        assert!(!g.connect(speed, speed, "Pose"));
        assert!(!g.connect(clip, out, "Nope"));
        assert!(g.remove(speed));
        assert_eq!(g.nodes[out].input("Pose"), None);
        assert!(!g.remove(out));
    }

    #[test]
    fn pins_grow_with_numbered_names_and_ids_are_unique() {
        let mut g = Graph::default();
        let sm = g.add(NodeKind::StateMachine);
        assert_eq!(g.nodes[sm].add_pin(), Some("State3"));
        let speed = g.add(NodeKind::Speed);
        assert_eq!(g.nodes[speed].add_pin(), None);
        let other = g.add(NodeKind::StateMachine);
        assert_ne!(g.nodes[sm].id, g.nodes[other].id);
        let blend = g.add(NodeKind::Blend1D);
        assert!(g.nodes[blend].inputs[0].props.contains_key("Position"));
    }

    #[test]
    fn values_coerce_like_luau() {
        assert!(Value::Number(0.).truthy());
        assert!(!Value::Bool(false).truthy());
        assert_eq!(Value::Text(" 2.5".into()).number(), 2.5);
        assert_eq!(Value::Bool(true).number(), 1.);
    }
}
