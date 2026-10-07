//! Every kind of node a graph can hold: its pins, where it sits in the add
//! menu, and the Luau it stands for. The table itself is in [`kinds`];
//! this module is what the rest of the graph reads it through.

mod kinds;

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
}

impl PinType {
    /// Whether a wire carrying `from` may end on a pin of this type. `Any`
    /// takes and gives every value, never the run order.
    pub(crate) fn accepts(self, from: PinType) -> bool {
        match (self, from) {
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
        }
    }
}

/// Luau operator precedence, low to high, as far as wrapping a
/// sub-expression in parentheses needs it. `Atom` is anything that can be
/// indexed or called without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prec {
    Or,
    And,
    Compare,
    Concat,
    Add,
    Mul,
    Unary,
    Pow,
    Atom,
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
}

impl Kind {
    pub(crate) fn input(&self, name: &str) -> Option<&'static Pin> {
        self.inputs.iter().find(|pin| pin.name == name)
    }

    pub(crate) fn output(&self, name: &str) -> Option<&'static Pin> {
        self.outputs.iter().find(|pin| pin.name == name)
    }

    pub(crate) fn is_event(&self) -> bool {
        matches!(self.code, Code::Event(_))
    }

    /// The type this node takes first, then gives first, for the add
    /// menu's `Instance → Instance` column. Run-order pins are left out.
    pub(crate) fn signature(&self) -> (Option<PinType>, Option<PinType>) {
        let first = |pins: &[Pin]| pins.iter().map(|p| p.ty).find(|&ty| ty != PinType::Exec);
        (first(self.inputs), first(self.outputs))
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
    pub(crate) fn pin(self, kind: &Kind) -> Option<&'static Pin> {
        match self {
            Wanted::Input(ty) => kind.inputs.iter().find(|pin| pin.ty.accepts(ty)),
            Wanted::Output(ty) => kind.outputs.iter().find(|pin| ty.accepts(pin.ty)),
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
