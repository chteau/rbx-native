//! The node table. Pins are listed in the order a node draws them: input
//! `n` shares a row with output `n` (see `graph::layout`), which is why a
//! run-order pin always comes first on both sides.

use super::{Category, Code, Kind, Pin, PinType, Prec};

const fn exec(name: &'static str) -> Pin {
    Pin {
        name,
        ty: PinType::Exec,
        default: None,
    }
}

const fn wired(name: &'static str, ty: PinType) -> Pin {
    Pin {
        name,
        ty,
        default: None,
    }
}

const fn literal(name: &'static str, ty: PinType, default: &'static str) -> Pin {
    Pin {
        name,
        ty,
        default: Some(default),
    }
}

const IN: Pin = exec("");
const THEN: Pin = exec("");

const fn expression(template: &'static str, prec: Prec) -> Code {
    Code::Expression {
        template,
        prec,
        call: false,
    }
}

const fn call(template: &'static str) -> Code {
    Code::Expression {
        template,
        prec: Prec::Atom,
        call: true,
    }
}

const fn node(
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
    }
}

/// A list of pins as a `'static` slice: a pin built by a `const fn` is not
/// promoted on its own when it sits in a call's argument.
macro_rules! pins {
    ($($pin:expr),* $(,)?) => {
        const { &[$($pin),*] }
    };
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
const fn binary(
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

use Category::*;
use PinType::{Any, Bool, Instance, List, Number, String};

pub(crate) const KINDS: &[Kind] = &[
    // Events.
    node(
        "start",
        "On Start",
        Events,
        pins![],
        pins![THEN],
        Code::Event(None),
    ),
    node(
        "touched",
        "Touched",
        Events,
        pins![literal("Part", Instance, "script.Parent")],
        pins![THEN, wired("hit", Instance)],
        Code::Event(Some("{Part}.Touched")),
    ),
    node(
        "touch_ended",
        "Touch Ended",
        Events,
        pins![literal("Part", Instance, "script.Parent")],
        pins![THEN, wired("hit", Instance)],
        Code::Event(Some("{Part}.TouchEnded")),
    ),
    node(
        "clicked",
        "Clicked",
        Events,
        pins![literal("Detector", Instance, "script.Parent")],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("{Detector}.MouseClick")),
    ),
    node(
        "player_added",
        "Player Added",
        Events,
        pins![],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("game:GetService(\"Players\").PlayerAdded")),
    ),
    node(
        "player_removing",
        "Player Removing",
        Events,
        pins![],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("game:GetService(\"Players\").PlayerRemoving")),
    ),
    node(
        "heartbeat",
        "Heartbeat",
        Events,
        pins![],
        pins![THEN, wired("dt", Number)],
        Code::Event(Some("game:GetService(\"RunService\").Heartbeat")),
    ),
    node(
        "child_added",
        "Child Added",
        Events,
        pins![literal("Parent", Instance, "script.Parent")],
        pins![THEN, wired("child", Instance)],
        Code::Event(Some("{Parent}.ChildAdded")),
    ),
    node(
        "attribute_changed",
        "Attribute Changed",
        Events,
        pins![
            literal("Instance", Instance, "script.Parent"),
            literal("Name", String, ""),
        ],
        pins![THEN],
        Code::Event(Some("{Instance}:GetAttributeChangedSignal({Name})")),
    ),
    // Flow.
    node(
        "branch",
        "Branch",
        Flow,
        pins![IN, wired("Condition", Bool)],
        pins![exec("True"), exec("False")],
        Code::Branch,
    ),
    node(
        "for_each",
        "For Each",
        Flow,
        pins![IN, wired("List", List)],
        pins![
            exec("Loop"),
            exec("Completed"),
            wired("Item", Any),
            wired("Index", Number)
        ],
        Code::ForEach,
    ),
    node(
        "repeat",
        "Repeat",
        Flow,
        pins![IN, literal("Count", Number, "3")],
        pins![exec("Loop"), exec("Completed"), wired("Index", Number)],
        Code::Repeat,
    ),
    node(
        "wait",
        "Wait",
        Flow,
        pins![IN, literal("Seconds", Number, "1")],
        pins![THEN],
        Code::Statement("task.wait({Seconds})"),
    ),
    // Instances.
    node(
        "get_parent",
        "Get Parent",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Parent", Instance)],
        expression("{Instance}.Parent", Prec::Atom),
    ),
    node(
        "get_children",
        "Get Children",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Children", List)],
        call("{Instance}:GetChildren()"),
    ),
    node(
        "get_descendants",
        "Get Descendants",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Descendants", List)],
        call("{Instance}:GetDescendants()"),
    ),
    Kind {
        names_local: Some("Name"),
        ..node(
            "find_first_child",
            "Find First Child",
            Instances,
            pins![wired("Parent", Instance), literal("Name", String, "")],
            pins![wired("Result", Instance)],
            call("{Parent}:FindFirstChild({Name})"),
        )
    },
    Kind {
        names_local: Some("Class"),
        ..node(
            "find_child_of_class",
            "Find Child Of Class",
            Instances,
            pins![
                wired("Parent", Instance),
                literal("Class", String, "Humanoid")
            ],
            pins![wired("Result", Instance)],
            call("{Parent}:FindFirstChildOfClass({Class})"),
        )
    },
    Kind {
        names_local: Some("Class"),
        ..node(
            "find_ancestor_of_class",
            "Find Ancestor Of Class",
            Instances,
            pins![
                wired("Instance", Instance),
                literal("Class", String, "Model")
            ],
            pins![wired("Result", Instance)],
            call("{Instance}:FindFirstAncestorOfClass({Class})"),
        )
    },
    Kind {
        names_local: Some("Name"),
        ..node(
            "wait_for_child",
            "Wait For Child",
            Instances,
            pins![wired("Parent", Instance), literal("Name", String, "")],
            pins![wired("Result", Instance)],
            call("{Parent}:WaitForChild({Name})"),
        )
    },
    node(
        "is_a",
        "Is A",
        Instances,
        pins![
            wired("Instance", Instance),
            literal("Class", String, "BasePart")
        ],
        pins![wired("Result", Bool)],
        call("{Instance}:IsA({Class})"),
    ),
    Kind {
        names_local: Some("Service"),
        ..node(
            "get_service",
            "Get Service",
            Instances,
            pins![literal("Service", String, "Players")],
            pins![wired("Service", Instance)],
            call("game:GetService({Service})"),
        )
    },
    node(
        "player_from_character",
        "Get Player From Character",
        Instances,
        pins![wired("Character", Instance)],
        pins![wired("Player", Instance)],
        call("game:GetService(\"Players\"):GetPlayerFromCharacter({Character})"),
    ),
    node(
        "clone",
        "Clone",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Clone", Instance)],
        call("{Instance}:Clone()"),
    ),
    node(
        "new_instance",
        "New Instance",
        Instances,
        pins![literal("Class", String, "Part")],
        pins![wired("Instance", Instance)],
        call("Instance.new({Class})"),
    ),
    node(
        "set_parent",
        "Set Parent",
        Instances,
        pins![
            IN,
            wired("Instance", Instance),
            literal("Parent", Instance, "workspace")
        ],
        pins![THEN],
        Code::Statement("{Instance}.Parent = {Parent}"),
    ),
    node(
        "destroy",
        "Destroy",
        Instances,
        pins![IN, wired("Instance", Instance)],
        pins![THEN],
        Code::Statement("{Instance}:Destroy()"),
    ),
    // Properties.
    node(
        "get_property",
        "Get Property",
        Properties,
        pins![
            wired("Target", Instance),
            literal("Property", String, "Name")
        ],
        pins![wired("Value", Any)],
        expression("{Target}{.Property}", Prec::Atom),
    ),
    node(
        "set_property",
        "Set Property",
        Properties,
        pins![
            IN,
            wired("Target", Instance),
            literal("Property", String, "Name"),
            literal("Value", Any, "nil")
        ],
        pins![THEN],
        Code::Statement("{Target}{.Property} = {Value}"),
    ),
    node(
        "get_attribute",
        "Get Attribute",
        Properties,
        pins![wired("Instance", Instance), literal("Name", String, "")],
        pins![wired("Value", Any)],
        call("{Instance}:GetAttribute({Name})"),
    ),
    node(
        "set_attribute",
        "Set Attribute",
        Properties,
        pins![
            IN,
            wired("Instance", Instance),
            literal("Name", String, ""),
            literal("Value", Any, "nil")
        ],
        pins![THEN],
        Code::Statement("{Instance}:SetAttribute({Name}, {Value})"),
    ),
    node(
        "get_tag",
        "Get Tag",
        Properties,
        pins![wired("Instance", Instance), literal("Tag", String, "")],
        pins![wired("Result", Bool)],
        call("{Instance}:HasTag({Tag})"),
    ),
    node(
        "add_tag",
        "Add Tag",
        Properties,
        pins![IN, wired("Instance", Instance), literal("Tag", String, "")],
        pins![THEN],
        Code::Statement("{Instance}:AddTag({Tag})"),
    ),
    node(
        "remove_tag",
        "Remove Tag",
        Properties,
        pins![IN, wired("Instance", Instance), literal("Tag", String, "")],
        pins![THEN],
        Code::Statement("{Instance}:RemoveTag({Tag})"),
    ),
    node(
        "get_name",
        "Get Name",
        Properties,
        pins![wired("Instance", Instance)],
        pins![wired("Name", String)],
        expression("{Instance}.Name", Prec::Atom),
    ),
    // Logic.
    node(
        "is_valid",
        "Is Valid",
        Logic,
        pins![wired("Value", Any)],
        pins![wired("Result", Bool)],
        expression("{Value} ~= nil", Prec::Compare),
    ),
    node(
        "not",
        "Not",
        Logic,
        pins![literal("Value", Bool, "false")],
        pins![wired("Result", Bool)],
        expression("not {Value}", Prec::Unary),
    ),
    binary("and", "And", Logic, Bool, Bool, "{A} and {B}", Prec::And),
    binary("or", "Or", Logic, Bool, Bool, "{A} or {B}", Prec::Or),
    binary(
        "equal",
        "Equal",
        Logic,
        Any,
        Bool,
        "{A} == {B}",
        Prec::Compare,
    ),
    binary(
        "not_equal",
        "Not Equal",
        Logic,
        Any,
        Bool,
        "{A} ~= {B}",
        Prec::Compare,
    ),
    binary(
        "greater",
        "Greater Than",
        Logic,
        Number,
        Bool,
        "{A} > {B}",
        Prec::Compare,
    ),
    binary(
        "less",
        "Less Than",
        Logic,
        Number,
        Bool,
        "{A} < {B}",
        Prec::Compare,
    ),
    binary(
        "at_least",
        "At Least",
        Logic,
        Number,
        Bool,
        "{A} >= {B}",
        Prec::Compare,
    ),
    binary(
        "at_most",
        "At Most",
        Logic,
        Number,
        Bool,
        "{A} <= {B}",
        Prec::Compare,
    ),
    node(
        "select",
        "Select",
        Logic,
        pins![
            literal("Condition", Bool, "false"),
            literal("True", Any, "nil"),
            literal("False", Any, "nil"),
        ],
        pins![wired("Result", Any)],
        expression("if {Condition} then {True} else {False}", Prec::Or),
    ),
    // Math.
    binary("add", "Add", Math, Number, Number, "{A} + {B}", Prec::Add),
    binary(
        "subtract",
        "Subtract",
        Math,
        Number,
        Number,
        "{A} - {B}",
        Prec::Add,
    ),
    binary(
        "multiply",
        "Multiply",
        Math,
        Number,
        Number,
        "{A} * {B}",
        Prec::Mul,
    ),
    binary(
        "divide",
        "Divide",
        Math,
        Number,
        Number,
        "{A} / {B}",
        Prec::Mul,
    ),
    binary(
        "modulo",
        "Modulo",
        Math,
        Number,
        Number,
        "{A} % {B}",
        Prec::Mul,
    ),
    binary(
        "power",
        "Power",
        Math,
        Number,
        Number,
        "{A} ^ {B}",
        Prec::Pow,
    ),
    binary(
        "min",
        "Min",
        Math,
        Number,
        Number,
        "math.min({A}, {B})",
        Prec::Atom,
    ),
    binary(
        "max",
        "Max",
        Math,
        Number,
        Number,
        "math.max({A}, {B})",
        Prec::Atom,
    ),
    node(
        "clamp",
        "Clamp",
        Math,
        pins![
            literal("Value", Number, "0"),
            literal("Min", Number, "0"),
            literal("Max", Number, "1"),
        ],
        pins![wired("Result", Number)],
        expression("math.clamp({Value}, {Min}, {Max})", Prec::Atom),
    ),
    node(
        "abs",
        "Absolute",
        Math,
        pins![literal("Value", Number, "0")],
        pins![wired("Result", Number)],
        expression("math.abs({Value})", Prec::Atom),
    ),
    node(
        "round",
        "Round",
        Math,
        pins![literal("Value", Number, "0")],
        pins![wired("Result", Number)],
        expression("math.round({Value})", Prec::Atom),
    ),
    node(
        "negate",
        "Negate",
        Math,
        pins![literal("Value", Number, "0")],
        pins![wired("Result", Number)],
        expression("-{Value}", Prec::Unary),
    ),
    node(
        "random",
        "Random",
        Math,
        pins![literal("Min", Number, "1"), literal("Max", Number, "10")],
        pins![wired("Result", Number)],
        call("math.random({Min}, {Max})"),
    ),
    // Values.
    node(
        "number",
        "Number",
        Values,
        pins![literal("Value", Number, "0")],
        pins![wired("Result", Number)],
        expression("{Value}", Prec::Atom),
    ),
    node(
        "string",
        "String",
        Values,
        pins![literal("Value", String, "")],
        pins![wired("Result", String)],
        expression("{Value}", Prec::Atom),
    ),
    node(
        "boolean",
        "Boolean",
        Values,
        pins![literal("Value", Bool, "true")],
        pins![wired("Result", Bool)],
        expression("{Value}", Prec::Atom),
    ),
    node(
        "instance",
        "Instance Path",
        Values,
        pins![literal("Path", Instance, "workspace")],
        pins![wired("Instance", Instance)],
        expression("{Path}", Prec::Atom),
    ),
    binary(
        "join",
        "Join Text",
        Values,
        String,
        String,
        "{A} .. {B}",
        Prec::Concat,
    ),
    node(
        "to_string",
        "To String",
        Values,
        pins![literal("Value", Any, "nil")],
        pins![wired("Result", String)],
        expression("tostring({Value})", Prec::Atom),
    ),
    node(
        "to_number",
        "To Number",
        Values,
        pins![literal("Value", Any, "nil")],
        pins![wired("Result", Number)],
        expression("tonumber({Value})", Prec::Atom),
    ),
    node(
        "vector3",
        "Vector3",
        Values,
        pins![
            literal("X", Number, "0"),
            literal("Y", Number, "0"),
            literal("Z", Number, "0"),
        ],
        pins![wired("Vector", Any)],
        expression("Vector3.new({X}, {Y}, {Z})", Prec::Atom),
    ),
    node(
        "color3",
        "Color From RGB",
        Values,
        pins![
            literal("R", Number, "255"),
            literal("G", Number, "255"),
            literal("B", Number, "255"),
        ],
        pins![wired("Color", Any)],
        expression("Color3.fromRGB({R}, {G}, {B})", Prec::Atom),
    ),
    node(
        "count",
        "Count",
        Values,
        pins![wired("List", List)],
        pins![wired("Count", Number)],
        expression("#{List}", Prec::Unary),
    ),
    // Output.
    node(
        "print",
        "Print",
        Output,
        pins![IN, literal("Value", Any, "\"Hello\"")],
        pins![THEN],
        Code::Statement("print({Value})"),
    ),
    node(
        "warn",
        "Warn",
        Output,
        pins![IN, literal("Value", Any, "\"Warning\"")],
        pins![THEN],
        Code::Statement("warn({Value})"),
    ),
];
