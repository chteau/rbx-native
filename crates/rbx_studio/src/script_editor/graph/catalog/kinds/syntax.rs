//! Generic Luau syntax: a node for every construct the named kinds do not
//! cover. Titles and pin names are plain language, not Luau's; what each
//! writes is in `codegen`.

use super::*;

/// A piece of code text typed on the node.
const fn word(name: &'static str, default: &'static str) -> Pin {
    literal(name, PinType::Word, default)
}

/// A value that is `default` until something is wired in.
const fn val(name: &'static str, default: &'static str) -> Pin {
    literal(name, PinType::Any, default)
}

/// A value that has to be wired.
const fn need(name: &'static str) -> Pin {
    wired(name, PinType::Any)
}

const VALUE: Pin = wired("Value", PinType::Any);

const fn syn(
    key: &'static str,
    title: &'static str,
    inputs: &'static [Pin],
    outputs: &'static [Pin],
    code: Syn,
) -> Kind {
    node(
        key,
        title,
        Category::Code,
        inputs,
        outputs,
        Code::Syntax(code),
    )
}

/// One repeating group of input pins.
macro_rules! repeat {
    ($count:expr, $min:expr, $($input:expr),+ $(,)?) => {
        Repeat { count: $count, inputs: pins![$($input),+], outputs: pins![], min: $min }
    };
}

macro_rules! repeats {
    ($($repeat:expr),* $(,)?) => {
        const { &[$($repeat),*] }
    };
}

pub(super) const KINDS: &[Kind] = &[
    syn(
        "local",
        "Make variable",
        pins![IN, word("Names", "x")],
        pins![THEN],
        Syn::Local,
    )
    .repeating(repeats![repeat!("#values", 0, val("Value", "nil"))]),
    syn("assign", "Set", pins![IN], pins![THEN], Syn::Assign).repeating(repeats![
        repeat!("#targets", 1, need("Target")),
        repeat!("#values", 1, val("Value", "nil")),
    ]),
    syn(
        "compound",
        "Change",
        pins![IN, need("Target"), word("Op", "+="), val("Value", "1")],
        pins![THEN],
        Syn::Compound,
    ),
    syn(
        "call",
        "Run function",
        pins![IN, need("Function")],
        pins![THEN],
        Syn::Call,
    )
    .repeating(repeats![repeat!("#args", 0, val("Input", "nil"))]),
    syn(
        "method",
        "Run method",
        pins![IN, need("Object"), word("Method", "Method")],
        pins![THEN],
        Syn::Method,
    )
    .repeating(repeats![repeat!("#args", 0, val("Input", "nil"))]),
    syn(
        "if",
        "If",
        pins![IN],
        pins![THEN, exec("Else")],
        Syn::If,
    )
    .repeating(repeats![Repeat {
        count: "#branches",
        inputs: pins![val("Condition", "true")],
        outputs: pins![exec("Then")],
        min: 1,
    }]),
    syn(
        "while",
        "Repeat while",
        pins![IN, val("Condition", "true")],
        pins![THEN, exec("Do")],
        Syn::While,
    ),
    syn(
        "repeat",
        "Repeat until",
        pins![IN, val("Condition", "true")],
        pins![THEN, exec("Do")],
        Syn::RepeatUntil,
    ),
    syn(
        "for_count",
        "Count",
        pins![
            IN,
            word("Variable", "i"),
            literal("From", PinType::Number, "1"),
            literal("To", PinType::Number, "10"),
            literal("Step", PinType::Any, ""),
        ],
        pins![THEN, exec("Do")],
        Syn::ForCount,
    ),
    syn(
        "for_each",
        "For each",
        pins![IN, word("Variables", "k, v")],
        pins![THEN, exec("Do")],
        Syn::ForIn,
    )
    .repeating(repeats![repeat!("#values", 1, need("In"))]),
    syn("do", "Block", pins![IN], pins![THEN, exec("Do")], Syn::Do),
    syn(
        "function",
        "Define function",
        pins![IN, word("Name", "name"), word("Parameters", "")],
        pins![THEN, exec("Body")],
        Syn::Function,
    ),
    syn("return", "Return", pins![IN], pins![], Syn::Return)
        .repeating(repeats![repeat!("#values", 0, val("Value", "nil"))]),
    syn("break", "Stop loop", pins![IN], pins![THEN], Syn::Break),
    syn(
        "continue",
        "Skip to next turn",
        pins![IN],
        pins![THEN],
        Syn::Continue,
    ),
    syn(
        "type",
        "Type",
        pins![IN, word("Text", "type T = any")],
        pins![THEN],
        Syn::Type,
    ),
    syn(
        "comment",
        "Note",
        pins![IN, word("Text", "-- note")],
        pins![THEN],
        Syn::Comment,
    ),
    // Values.
    syn("get", "Variable", pins![word("Name", "x")], pins![VALUE], Syn::Get),
    syn(
        "field",
        "Get field",
        pins![need("Object"), word("Name", "Name")],
        pins![VALUE],
        Syn::Field,
    ),
    syn(
        "index",
        "Get item",
        pins![need("Object"), need("Key")],
        pins![VALUE],
        Syn::Index,
    ),
    syn(
        "call_value",
        "Function result",
        pins![need("Function")],
        pins![VALUE],
        Syn::CallValue,
    )
    .repeating(repeats![repeat!("#args", 0, val("Input", "nil"))]),
    syn(
        "method_value",
        "Method result",
        pins![need("Object"), word("Method", "Method")],
        pins![VALUE],
        Syn::MethodValue,
    )
    .repeating(repeats![repeat!("#args", 0, val("Input", "nil"))]),
    syn(
        "literal",
        "Value",
        pins![word("Text", "nil")],
        pins![VALUE],
        Syn::Literal,
    ),
    syn(
        "binary",
        "Operator",
        pins![val("A", "0"), word("Op", "+"), val("B", "0")],
        pins![VALUE],
        Syn::Binary,
    ),
    syn(
        "unary",
        "Operator (one side)",
        pins![word("Op", "not"), val("Value", "true")],
        pins![VALUE],
        Syn::Unary,
    ),
    syn(
        "paren",
        "Brackets",
        pins![val("Value", "nil")],
        pins![VALUE],
        Syn::Paren,
    ),
    syn(
        "function_value",
        "Function",
        pins![word("Parameters", "")],
        pins![exec("Body"), VALUE],
        Syn::FunctionValue,
    ),
    syn("table", "Table", pins![], pins![VALUE], Syn::Table)
        .repeating(repeats![repeat!("#fields", 0, val("Item", "nil"))]),
    syn(
        "pair",
        "Keyed item",
        pins![val("Key", "key"), val("Value", "nil")],
        pins![VALUE],
        Syn::Pair,
    ),
    syn("if_value", "Choose", pins![val("Else", "nil")], pins![VALUE], Syn::IfValue)
        .repeating(repeats![Repeat {
            count: "#branches",
            inputs: pins![val("Condition", "true"), val("Then", "nil")],
            outputs: pins![],
            min: 1,
        }]),
    syn(
        "interp",
        "Text with values",
        pins![],
        pins![VALUE],
        Syn::Interp,
    )
    .repeating(repeats![repeat!("#parts", 0, val("Value", "nil"))]),
    syn(
        "cast",
        "As type",
        pins![val("Value", "nil"), word("Type", "any")],
        pins![VALUE],
        Syn::Cast,
    ),
];
