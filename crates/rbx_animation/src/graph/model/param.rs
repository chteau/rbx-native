//! A graph's parameters: their types, defaults and names, and the green
//! reference nodes that draw them on the canvas.

use super::enums::{self, EnumType};
use super::Value;

/// A parameter's type, as the `AnimGraph_ParameterType` attribute names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamKind {
    #[default]
    Number,
    Bool,
    Text,
    /// A bool that resets itself after one evaluation.
    Trigger,
    /// Names one of the graph's shared masks.
    Mask,
    /// The enum types the editor makes a parameter of when a wire is dragged
    /// out of an enum property's pin.
    PlayMode,
    InputMode,
    PhaseSync,
}

impl ParamKind {
    /// The kinds the "New Parameter" menu offers, in its order.
    pub const NEW: [ParamKind; 5] = [
        ParamKind::Text,
        ParamKind::Bool,
        ParamKind::Number,
        ParamKind::Mask,
        ParamKind::Trigger,
    ];

    /// `AnimGraph_ParameterType`.
    pub fn type_name(self) -> &'static str {
        match self {
            ParamKind::Number => "Number",
            ParamKind::Bool => "Boolean",
            ParamKind::Text => "String",
            ParamKind::Trigger => "Trigger",
            ParamKind::Mask => "Mask",
            ParamKind::PlayMode => "PlayMode",
            ParamKind::InputMode => "InputMode",
            ParamKind::PhaseSync => "PhaseSync",
        }
    }

    pub fn from_type_name(name: &str) -> Option<ParamKind> {
        [
            ParamKind::Number,
            ParamKind::Bool,
            ParamKind::Text,
            ParamKind::Trigger,
            ParamKind::Mask,
            ParamKind::PlayMode,
            ParamKind::InputMode,
            ParamKind::PhaseSync,
        ]
        .into_iter()
        .find(|k| k.type_name() == name)
    }

    /// The menu text: lowercase for the three Luau types, as the editor has it.
    pub fn label(self) -> &'static str {
        match self {
            ParamKind::Number => "number",
            ParamKind::Bool => "boolean",
            ParamKind::Text => "string",
            other => other.type_name(),
        }
    }

    pub fn default_value(self) -> Value {
        match self {
            ParamKind::Number => Value::Number(0.),
            ParamKind::Bool | ParamKind::Trigger => Value::Bool(false),
            ParamKind::Text | ParamKind::Mask => Value::Text(String::new()),
            ParamKind::PlayMode => Value::Text("Loop".into()),
            ParamKind::InputMode => Value::Text("Cartesian".into()),
            ParamKind::PhaseSync => Value::Text("Synced".into()),
        }
    }

    pub fn enum_type(self) -> Option<&'static EnumType> {
        match self {
            ParamKind::PlayMode => Some(&enums::PLAY_MODE),
            ParamKind::InputMode => Some(&enums::INPUT_MODE),
            ParamKind::PhaseSync => Some(&enums::PHASE_SYNC),
            _ => None,
        }
    }

    /// The type a parameter made by dragging out of `prop`'s pin gets.
    pub fn for_prop(prop: &str, current: &Value) -> ParamKind {
        match prop {
            "PlayMode" => ParamKind::PlayMode,
            "InputMode" => ParamKind::InputMode,
            "PhaseSync" => ParamKind::PhaseSync,
            "Mask" => ParamKind::Mask,
            _ => match current {
                Value::Number(_) => ParamKind::Number,
                Value::Bool(_) => ParamKind::Bool,
                Value::Text(_) => ParamKind::Text,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
    pub default: Value,
}

/// One green pill on the canvas standing for a parameter; a parameter can
/// have several, each its own `Parameter_<name>` folder.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamRef {
    pub param: String,
    pub position: (f32, f32),
}

/// A name as the parameter pane accepts it: whitespace becomes `_` and a
/// leading `RBX_` goes, since the runtime ignores attributes with it.
pub fn sanitize_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    while let Some(rest) = out.strip_prefix("RBX_") {
        out = rest.to_owned();
    }
    out
}

/// `base`, or `base1`, `base2`, … — the first `taken` does not refuse. This
/// is the editor's `"%s%d"` de-duplication for nodes, pins and parameters.
pub fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    (1..)
        .map(|n| format!("{base}{n}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}
