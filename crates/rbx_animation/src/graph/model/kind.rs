//! `Enum.AnimationNodeType` and what each kind starts with.

use super::Value;

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
