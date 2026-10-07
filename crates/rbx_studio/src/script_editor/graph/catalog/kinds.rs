//! The node table. Pins are listed in the order a node draws them: input
//! `n` shares a row with output `n` (see `graph::layout`), which is why a
//! run-order pin always comes first on both sides.

use super::{Category, Code, Kind, Pin, PinType, Prec, Repeat, Syn};

/// A list of pins as a `'static` slice: a pin built by a `const fn` is not
/// promoted on its own when it sits in a call's argument.
macro_rules! pins {
    ($($pin:expr),* $(,)?) => {
        const { &[$($pin),*] }
    };
}

mod events;
mod instances;
mod syntax;
mod values;

pub(super) const fn exec(name: &'static str) -> Pin {
    Pin {
        name,
        ty: PinType::Exec,
        default: None,
    }
}

pub(super) const fn wired(name: &'static str, ty: PinType) -> Pin {
    Pin {
        name,
        ty,
        default: None,
    }
}

pub(super) const fn literal(name: &'static str, ty: PinType, default: &'static str) -> Pin {
    Pin {
        name,
        ty,
        default: Some(default),
    }
}

pub(super) const IN: Pin = exec("");
pub(super) const THEN: Pin = exec("");

pub(super) const fn expression(template: &'static str, prec: Prec) -> Code {
    Code::Expression { template, prec }
}

pub(super) const fn call(template: &'static str) -> Code {
    Code::Expression {
        template,
        prec: Prec::Atom,
    }
}

pub(super) const fn node(
    key: &'static str,
    title: &'static str,
    category: Category,
    inputs: &'static [Pin],
    outputs: &'static [Pin],
    code: Code,
) -> Kind {
    Kind {
        key,
        title,
        category,
        inputs,
        outputs,
        code,
        names_local: None,
        repeats: &[],
    }
}

const NUMBERS: [Pin; 2] = [
    literal("A", PinType::Number, "0"),
    literal("B", PinType::Number, "0"),
];
const BOOLS: [Pin; 2] = [
    literal("A", PinType::Bool, "false"),
    literal("B", PinType::Bool, "false"),
];
const STRINGS: [Pin; 2] = [
    literal("A", PinType::String, ""),
    literal("B", PinType::String, ""),
];
const VALUES: [Pin; 2] = [
    literal("A", PinType::Any, "nil"),
    literal("B", PinType::Any, "nil"),
];

/// A two-input operator, `A op B`, on `ty`.
pub(super) const fn binary(
    key: &'static str,
    title: &'static str,
    category: Category,
    ty: PinType,
    out: PinType,
    template: &'static str,
    prec: Prec,
) -> Kind {
    node(
        key,
        title,
        category,
        match ty {
            PinType::Number => &NUMBERS,
            PinType::Bool => &BOOLS,
            PinType::String => &STRINGS,
            _ => &VALUES,
        },
        match out {
            PinType::Bool => pins![wired("Result", PinType::Bool)],
            PinType::String => pins![wired("Result", PinType::String)],
            _ => pins![wired("Result", PinType::Number)],
        },
        expression(template, prec),
    )
}

/// The table, section by section, in add-menu order.
pub(crate) const SECTIONS: [&[Kind]; 4] = [
    events::KINDS,
    instances::KINDS,
    values::KINDS,
    syntax::KINDS,
];

impl Kind {
    /// This kind with pins that come in a number.
    pub(super) const fn repeating(mut self, repeats: &'static [Repeat]) -> Kind {
        self.repeats = repeats;
        self
    }
}
