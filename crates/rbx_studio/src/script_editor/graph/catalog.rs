//! Every kind of node a graph can hold: its pins, where it sits in the add
//! menu, and the Luau it stands for. The table itself is in [`kinds`];
//! this module is what the rest of the graph reads it through.

mod kinds;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// Every kind, in add-menu order.
pub(crate) fn all() -> impl Iterator<Item = &'static Kind> {
    kinds::SECTIONS.iter().flat_map(|section| section.iter())
}

pub(crate) fn count() -> usize {
    kinds::SECTIONS.iter().map(|section| section.len()).sum()
}

/// What travels along a wire. `Exec` is the order statements run in; every
/// other type is a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinType {
    Exec,
    Bool,
    Number,
    String,
    Instance,
    List,
    Any,
    /// A piece of code text typed on the node (a name, an operator, a
    /// type, a comment) and written as it stands. Never wired.
    Word,
}

impl PinType {
    /// Whether a wire carrying `from` may end on a pin of this type. `Any`
    /// takes and gives every value, never the run order or a word.
    pub(crate) fn accepts(self, from: PinType) -> bool {
        match (self, from) {
            (PinType::Word, _) | (_, PinType::Word) => false,
            (PinType::Exec, _) | (_, PinType::Exec) => self == from,
            (PinType::Any, _) | (_, PinType::Any) => true,
            _ => self == from,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            PinType::Exec => "exec",
            PinType::Bool => "boolean",
            PinType::Number => "number",
            PinType::String => "string",
            PinType::Instance => "Instance",
            PinType::List => "list",
            PinType::Any => "any",
            PinType::Word => "text",
        }
    }
}

/// One pin. A value input's `default` is the literal it holds while
/// nothing is wired into it; `None` means it must be wired.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pin {
    pub(crate) name: &'static str,
    pub(crate) ty: PinType,
    pub(crate) default: Option<&'static str>,
}

/// The add menu's sections, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Category {
    Events,
    Flow,
    Instances,
    Properties,
    Logic,
    Math,
    Values,
    Output,
    /// Generic Luau syntax: what the other kinds do not cover by name.
    Code,
}

impl Category {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::Events => "Events",
            Category::Flow => "Flow",
            Category::Instances => "Instances",
            Category::Properties => "Properties",
            Category::Logic => "Logic",
            Category::Math => "Math",
            Category::Values => "Values",
            Category::Output => "Output",
            Category::Code => "Code",
        }
    }
}

/// Luau operator precedence, low to high, as far as wrapping a
/// sub-expression in parentheses needs it. `Atom` is anything that can be
/// indexed or called without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prec {
    /// `if a then b else c` as a value: binds looser than everything.
    If,
    Or,
    And,
    Compare,
    Concat,
    Add,
    Mul,
    Unary,
    Pow,
    /// `x :: T` binds tighter than every operator.
    Cast,
    Atom,
}

impl Prec {
    /// `..` and `^` group to the right, as does a prefix operator.
    pub(crate) fn right_assoc(self) -> bool {
        matches!(self, Prec::Concat | Prec::Pow | Prec::Unary)
    }
}

/// The Luau a node stands for. Templates name inputs as `{Pin}`; `{.Pin}`
/// is an index, written `.Name` when the input is a literal identifier and
/// `[expr]` otherwise.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Code {
    /// A signal connected at the top level; its value outputs are the
    /// handler's parameters. `None` is the script starting: its body runs
    /// at the top level itself.
    Event(Option<&'static str>),
    /// A statement in a run, carried on by its exec output.
    Statement(&'static str),
    /// A value. `call` marks one that does work or can change between
    /// reads (a lookup, a clone), so it is read once into a local rather
    /// than repeated at every use.
    Expression {
        template: &'static str,
        prec: Prec,
        call: bool,
    },
    Branch,
    ForEach,
    Repeat,
    /// Luau kept as written: its `Code` input, line for line, re-indented
    /// to where the run puts it. What an import cannot draw as nodes
    /// becomes one of these, so no code is ever dropped.
    Raw,
    /// A piece of Luau syntax with no template of its own: written by
    /// `codegen`, which knows how each reads.
    Syntax(Syn),
}

/// The generic syntax kinds. Statements first, then values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Syn {
    Local,
    Assign,
    Compound,
    Call,
    Method,
    If,
    While,
    RepeatUntil,
    ForCount,
    ForIn,
    Do,
    Function,
    Return,
    Break,
    Continue,
    Type,
    Comment,
    Get,
    Field,
    Index,
    CallValue,
    MethodValue,
    Literal,
    Binary,
    Unary,
    Paren,
    FunctionValue,
    Table,
    Pair,
    IfValue,
    Interp,
    Cast,
}

impl Syn {
    /// Whether this is a value rather than a statement.
    pub(crate) fn is_value(self) -> bool {
        self as u8 >= Syn::Get as u8
    }
}

/// Pins that come in a number, kept in the node's `values[count]`
/// (`#args`): instance `i` of a pin named `Input` is `Input 1`, `Input 2`...
#[derive(Debug, Clone, Copy)]
pub(crate) struct Repeat {
    pub(crate) count: &'static str,
    pub(crate) inputs: &'static [Pin],
    pub(crate) outputs: &'static [Pin],
    /// How many a node has while `count` is unset.
    pub(crate) min: usize,
}

/// The pins one node has: its kind's, then each repeat's, as many as it holds.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pins {
    pub(crate) inputs: Vec<Pin>,
    pub(crate) outputs: Vec<Pin>,
}

impl Pins {
    pub(crate) fn input(&self, name: &str) -> Option<Pin> {
        self.inputs.iter().find(|pin| pin.name == name).copied()
    }

    pub(crate) fn output(&self, name: &str) -> Option<Pin> {
        self.outputs.iter().find(|pin| pin.name == name).copied()
    }

    /// Rows a node draws: input `n` shares one with output `n`.
    pub(crate) fn rows(&self) -> usize {
        self.inputs.len().max(self.outputs.len()).max(1)
    }
}

/// `"Input 2"` as a `'static` name. ponytail: leaked, bounded by the
/// distinct names a graph ever holds; a pool if that ever grows.
fn numbered(base: &str, i: usize) -> &'static str {
    static POOL: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let name = format!("{base} {i}");
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    match pool.get(name.as_str()) {
        Some(name) => name,
        None => {
            let name: &'static str = Box::leak(name.into_boxed_str());
            pool.insert(name);
            name
        }
    }
}

#[derive(Debug)]
pub(crate) struct Kind {
    pub(crate) key: &'static str,
    pub(crate) title: &'static str,
    pub(crate) category: Category,
    pub(crate) inputs: &'static [Pin],
    pub(crate) outputs: &'static [Pin],
    pub(crate) code: Code,
    /// The input whose literal names the local a value is read into
    /// (`FindFirstChildOfClass("Humanoid")` becomes `humanoid`).
    pub(crate) names_local: Option<&'static str>,
    pub(crate) repeats: &'static [Repeat],
}

impl Kind {
    /// The pins of a node of this kind holding `values`.
    pub(crate) fn pins(&self, values: &BTreeMap<String, String>) -> Pins {
        self.pins_with(|repeat| repeat.count_in(values))
    }

    /// The pins a node of this kind has with every repeat at least once,
    /// for the add menu to find one a dragged wire fits.
    pub(crate) fn sample_pins(&self) -> Pins {
        self.pins_with(|repeat| repeat.min.max(1))
    }

    fn pins_with(&self, count: impl Fn(&Repeat) -> usize) -> Pins {
        let mut pins = Pins {
            inputs: self.inputs.to_vec(),
            outputs: self.outputs.to_vec(),
        };
        for repeat in self.repeats {
            for i in 1..=count(repeat) {
                for (list, from) in [
                    (&mut pins.inputs, repeat.inputs),
                    (&mut pins.outputs, repeat.outputs),
                ] {
                    list.extend(from.iter().map(|pin| Pin {
                        name: numbered(pin.name, i),
                        ..*pin
                    }));
                }
            }
        }
        pins
    }

    pub(crate) fn input(&self, name: &str) -> Option<&'static Pin> {
        self.inputs.iter().find(|pin| pin.name == name)
    }

    pub(crate) fn is_event(&self) -> bool {
        matches!(self.code, Code::Event(_))
    }

    /// The type this node takes first, then gives first, for the add
    /// menu's `Instance → Instance` column. Run-order pins are left out.
    pub(crate) fn signature(&self) -> (Option<PinType>, Option<PinType>) {
        let first = |pins: &[Pin]| {
            pins.iter()
                .map(|p| p.ty)
                .find(|&ty| ty != PinType::Exec && ty != PinType::Word)
        };
        let sample = self.sample_pins();
        (first(&sample.inputs), first(&sample.outputs))
    }
}

impl Repeat {
    /// How many instances `values` asks for: `count`, never under `min`.
    pub(crate) fn count_in(&self, values: &BTreeMap<String, String>) -> usize {
        values
            .get(self.count)
            .and_then(|n| n.trim().parse().ok())
            .map_or(self.min, |n: usize| n.max(self.min))
    }
}

pub(crate) fn kind(key: &str) -> Option<&'static Kind> {
    all().find(|kind| kind.key == key)
}

/// Which way a wire being dragged from a pin still has to go: the menu
/// offers only nodes with a pin it can end on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// A wire from an output: nodes with an input that accepts it.
    Input(PinType),
    /// A wire from an input: nodes with an output it accepts.
    Output(PinType),
}

impl Wanted {
    /// The pin on `kind` a wire wanting this would end on: the first that
    /// fits.
    pub(crate) fn pin(self, kind: &Kind) -> Option<Pin> {
        let pins = kind.sample_pins();
        match self {
            Wanted::Input(ty) => pins.inputs.into_iter().find(|pin| pin.ty.accepts(ty)),
            Wanted::Output(ty) => pins.outputs.into_iter().find(|pin| ty.accepts(pin.ty)),
        }
    }
}

/// The add menu's rows for `query`: every kind whose title holds each word
/// of it, narrowed to the ones `wanted` fits, in menu order.
pub(crate) fn search(query: &str, wanted: Option<Wanted>) -> Vec<&'static Kind> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut found: Vec<&'static Kind> = all()
        .filter(|kind| {
            let title = kind.title.to_lowercase();
            words.iter().all(|word| title.contains(word.as_str()))
        })
        .filter(|kind| wanted.is_none_or(|wanted| wanted.pin(kind).is_some()))
        .collect();
    // Stable, so the table's own order holds within a section.
    found.sort_by_key(|kind| kind.category);
    found
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
