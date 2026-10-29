//! `Enum.AnimationNodeType` and what each kind starts with, as the shipping
//! Graph Editor creates it.

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

/// The editor's own transition length, used wherever a node's transition
/// attribute is unset.
pub const DEFAULT_TRANSITION_DURATION: f64 = 0.1;

impl NodeKind {
    /// Every kind a builder can insert, in the Insert Node menu's order
    /// (grouped as [`NodeKind::group`] says). The output node always exists.
    pub const INSERTABLE: [NodeKind; 14] = [
        NodeKind::Clip,
        NodeKind::PrioritySelect,
        NodeKind::Select,
        NodeKind::StateMachine,
        NodeKind::RandomSequence,
        NodeKind::Sequence,
        NodeKind::Add,
        NodeKind::Blend1D,
        NodeKind::Blend2D,
        NodeKind::Over,
        NodeKind::Subtract,
        NodeKind::OneShot,
        NodeKind::Speed,
        NodeKind::Mask,
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

    /// The `AnimationNodeType` item name, which is also the stem of a new
    /// node's instance name (`ClipNode`, `ClipNode1`, …).
    pub fn type_name(self) -> &'static str {
        match self {
            NodeKind::Add => "AddNode",
            NodeKind::Over => "OverNode",
            NodeKind::Blend1D => "Blend1DNode",
            NodeKind::Blend2D => "Blend2DNode",
            NodeKind::Clip => "ClipNode",
            NodeKind::GraphOutput => "GraphOutput",
            NodeKind::Mask => "MaskNode",
            NodeKind::PrioritySelect => "PrioritySelectNode",
            NodeKind::RandomSequence => "RandomSequenceNode",
            NodeKind::Select => "SelectNode",
            NodeKind::Sequence => "SequenceNode",
            NodeKind::Speed => "SpeedNode",
            NodeKind::Subtract => "SubtractNode",
            NodeKind::OneShot => "OneShotNode",
            NodeKind::StateMachine => "StateMachineNode",
        }
    }

    /// The header text: the type name without "Node", spaced before each
    /// capital that follows a lowercase letter (so "Blend1D" stays whole).
    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Add => "Add",
            NodeKind::Over => "Over",
            NodeKind::Blend1D => "Blend1D",
            NodeKind::Blend2D => "Blend2D",
            NodeKind::Clip => "Clip",
            NodeKind::GraphOutput => "Graph Output",
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

    /// The Insert Node menu group. Expression (a value node, not a kind
    /// here) sits in "Misc" after Mask.
    pub fn group(self) -> &'static str {
        match self {
            NodeKind::Clip => "Content",
            NodeKind::PrioritySelect | NodeKind::Select | NodeKind::StateMachine => "State",
            NodeKind::RandomSequence | NodeKind::Sequence => "Sequence",
            NodeKind::Add
            | NodeKind::Blend1D
            | NodeKind::Blend2D
            | NodeKind::Over
            | NodeKind::Subtract
            | NodeKind::OneShot => "Blend",
            NodeKind::Speed | NodeKind::Mask => "Misc",
            NodeKind::GraphOutput => "Output",
        }
    }

    /// The pins a new node starts with. Fixed pins have the engine's names;
    /// the kinds that take a list of inputs start empty, except Blend1D,
    /// which the editor seeds with A and B.
    pub fn default_pins(self) -> &'static [&'static str] {
        match self {
            NodeKind::GraphOutput | NodeKind::Speed | NodeKind::Mask => &["Pose"],
            NodeKind::Over => &["Base", "Over"],
            NodeKind::Add => &["Base", "Additive"],
            NodeKind::Subtract => &["A", "B"],
            NodeKind::OneShot => &["Base", "OneShot"],
            NodeKind::Blend1D => &["A", "B"],
            NodeKind::Clip
            | NodeKind::StateMachine
            | NodeKind::Blend2D
            | NodeKind::Select
            | NodeKind::PrioritySelect
            | NodeKind::Sequence
            | NodeKind::RandomSequence => &[],
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

    /// The node's own properties and their defaults, by the attribute names
    /// the editor writes. Enum values are item names.
    pub fn default_props(self) -> Vec<(&'static str, Value)> {
        use Value::{Bool, Number, Text};
        let text = |s: &str| Text(s.to_owned());
        let duration = || {
            (
                "DefaultTransitionDuration",
                Number(DEFAULT_TRANSITION_DURATION),
            )
        };
        let curve = || ("DefaultTransitionCurve", text("Linear"));
        let when = || ("DefaultTransitionWhen", text("Finished"));
        match self {
            NodeKind::Clip => vec![
                ("AnimationId", text("")),
                ("PlayMode", text("Loop")),
                ("Reverse", Bool(false)),
                ("Speed", Number(1.)),
                ("Trim", Bool(false)),
                ("TrimStart", Number(0.)),
                ("TrimEnd", Number(0.)),
            ],
            NodeKind::Speed => vec![("Speed", Number(1.))],
            NodeKind::Over | NodeKind::Add | NodeKind::Subtract => vec![("Weight", Number(1.))],
            // `Mask` names one of the graph's shared masks; storage keeps it
            // as an ObjectValue rather than an attribute.
            NodeKind::Mask => vec![("Mask", text("")), ("Invert", Bool(false))],
            NodeKind::Blend1D => vec![("PhaseSync", text("Synced")), ("Position", Number(0.))],
            NodeKind::Blend2D => vec![
                ("PhaseSync", text("Synced")),
                ("InputMode", text("Cartesian")),
                ("X", Number(0.)),
                ("Y", Number(0.)),
            ],
            NodeKind::Select => vec![("Selection", text("")), duration(), curve()],
            NodeKind::PrioritySelect => {
                vec![
                    ("DefaultInterruptible", text("Always")),
                    duration(),
                    curve(),
                ]
            }
            NodeKind::Sequence => vec![
                ("LoopCount", Number(0.)),
                ("DefaultWaitFor", text("Finished")),
                duration(),
                curve(),
                when(),
            ],
            NodeKind::RandomSequence => vec![
                ("PlayCount", Number(0.)),
                ("Seed", Number(-1.)),
                duration(),
                curve(),
                when(),
            ],
            NodeKind::OneShot => vec![
                ("Trigger", Bool(false)),
                ("Cancel", Bool(false)),
                ("Interruptible", Bool(true)),
                ("BlendMode", text("Over")),
                ("TransitionInDuration", Number(0.15)),
                ("TransitionInCurve", text("Linear")),
                ("TransitionOutDuration", Number(0.15)),
                ("TransitionOutCurve", text("Linear")),
                ("TransitionOutWhen", text("Finished")),
            ],
            // The entry state's pin name; one naming no state plays nothing.
            NodeKind::StateMachine => vec![("EntryState", text(""))],
            NodeKind::GraphOutput => vec![],
        }
    }

    /// How the editor shows a number property's special value: LoopCount
    /// and PlayCount 0 as "0 (Infinity)", Seed −1 as "-1 (Random)".
    pub fn number_label(prop: &str, value: f64) -> Option<&'static str> {
        match prop {
            "LoopCount" | "PlayCount" if value == 0. => Some("0 (Infinity)"),
            "Seed" if value == -1. => Some("-1 (Random)"),
            _ => None,
        }
    }

    /// Settings each input pin of this kind carries. The
    /// `TransitionOverride*` settings are absent until set, which is what
    /// makes the node's default transition apply.
    pub fn default_input_props(self) -> Vec<(&'static str, Value)> {
        use Value::{Bool, Number};
        match self {
            NodeKind::Blend1D => vec![("Position", Number(0.))],
            NodeKind::Blend2D => vec![("X", Number(0.)), ("Y", Number(0.))],
            NodeKind::PrioritySelect => {
                vec![
                    ("Trigger", Bool(false)),
                    ("InterruptibleTrigger", Bool(false)),
                ]
            }
            NodeKind::Sequence => vec![("WaitForTrigger", Bool(false))],
            NodeKind::RandomSequence => vec![("Weight", Number(1.))],
            _ => vec![],
        }
    }
}
