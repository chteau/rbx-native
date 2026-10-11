//! One node of a graph, its input pins, and how pins are added and named.

use std::collections::BTreeMap;

use super::param::unique;
use super::{NodeKind, Prop, Transition, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// The pin name; for a state machine, the state's name.
    pub pin: String,
    /// Index into [`super::Graph::nodes`].
    pub node: Option<usize>,
    pub props: BTreeMap<String, Prop>,
    /// A state's place in the state-machine view.
    pub position: Option<(f32, f32)>,
}

impl Input {
    fn new(kind: NodeKind, pin: &str) -> Input {
        Input {
            pin: pin.to_owned(),
            node: None,
            props: values(kind.default_input_props()),
            position: None,
        }
    }
}

fn values(list: Vec<(&'static str, Value)>) -> BTreeMap<String, Prop> {
    list.into_iter()
        .map(|(k, v)| (k.to_owned(), Prop::Value(v)))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// The instance name (`ClipNode`, `ClipNode1`, …): unique within the
    /// graph, and what pins and transitions are keyed on in storage.
    pub id: String,
    /// What the header shows (`AnimGraph_DisplayName`); the kind's label
    /// when the node has not been renamed.
    pub name: String,
    pub kind: NodeKind,
    pub props: BTreeMap<String, Prop>,
    pub inputs: Vec<Input>,
    pub transitions: Vec<Transition>,
    /// Where the node editor draws its top-left corner.
    pub position: (f32, f32),
    /// Set once the node has been resized.
    pub size: Option<(f32, f32)>,
    pub collapsed: bool,
    pub z_index: f64,
    /// The `InputPinData` blob read from a file, with the pin names it was
    /// read beside. Its layout is undocumented, so it is written back only
    /// while those pins are unchanged.
    pub(crate) pin_data: Option<(Vec<String>, Vec<u8>)>,
}

impl Node {
    pub fn new(kind: NodeKind, id: &str) -> Node {
        let mut inputs: Vec<Input> = kind
            .default_pins()
            .iter()
            .map(|pin| Input::new(kind, pin))
            .collect();
        if kind == NodeKind::Blend1D {
            inputs[1]
                .props
                .insert("Position".into(), Prop::Value(Value::Number(1.)));
        }
        Node {
            id: id.to_owned(),
            name: kind.label().to_owned(),
            kind,
            props: values(kind.default_props()),
            inputs,
            transitions: Vec::new(),
            position: (0., 0.),
            size: None,
            collapsed: false,
            z_index: 0.,
            pin_data: None,
        }
    }

    pub fn input(&self, pin: &str) -> Option<usize> {
        self.inputs.iter().find(|i| i.pin == pin)?.node
    }

    /// Whether the node has been given its own display name.
    pub fn renamed(&self) -> bool {
        self.name != self.kind.label()
    }

    /// Adds an unwired pin and returns its name: "New State" (then "New
    /// State1", …) on a state machine, else `Input<n>` counting from the
    /// pins there are, as the editor's placeholder row is numbered.
    pub fn add_pin(&mut self) -> Option<&str> {
        if !self.kind.grows_pins() {
            return None;
        }
        let name = if self.kind == NodeKind::StateMachine {
            unique("New State", |n| self.has_pin(n))
        } else {
            let start = self.inputs.len();
            (start..)
                .map(|n| format!("Input{n}"))
                .find(|n| !self.has_pin(n))
                .unwrap_or_default()
        };
        self.push_pin(&name);
        self.inputs.last().map(|i| i.pin.as_str())
    }

    pub(super) fn push_pin(&mut self, name: &str) {
        self.inputs.push(Input::new(self.kind, name));
    }

    fn has_pin(&self, name: &str) -> bool {
        self.inputs.iter().any(|i| i.pin == name)
    }

    /// Removes a dynamic pin, and on a state machine every transition to or
    /// from that state.
    pub fn remove_pin(&mut self, pin: &str) -> bool {
        let Some(at) = self
            .inputs
            .iter()
            .position(|i| i.pin == pin)
            .filter(|_| self.kind.grows_pins())
        else {
            return false;
        };
        self.inputs.remove(at);
        let shift = |i: usize| if i > at { i - 1 } else { i };
        self.transitions
            .retain(|t| t.to != at && t.from != Some(at));
        for t in &mut self.transitions {
            t.to = shift(t.to);
            t.from = t.from.map(shift);
        }
        true
    }

    /// The name a pin fed by this node gets: its display name, or its
    /// instance name less the `Node<n>` suffix ("ClipNode1" → "Clip").
    pub(super) fn pin_stem(&self) -> String {
        if self.renamed() {
            return self.name.clone();
        }
        let digits = self.id.trim_end_matches(|c: char| c.is_ascii_digit());
        digits.strip_suffix("Node").unwrap_or(digits).to_owned()
    }

    pub(super) fn unique_pin(&self, stem: &str) -> String {
        unique(stem, |n| self.has_pin(n))
    }
}
