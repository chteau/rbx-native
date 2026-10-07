//! The generic syntax kinds, operator brackets, splicing and anchors.

use super::super::catalog;
use super::super::{End, Graph};
use super::{anchors, compile, compile_with, fingerprint, Origin, Origins};

fn add(g: &mut Graph, key: &str) -> u32 {
    g.add(catalog::kind(key).unwrap(), [0.0, 0.0])
}

fn wire(g: &mut Graph, from: (u32, &str), to: (u32, &str)) {
    g.connect(End::new(from.0, from.1), End::new(to.0, to.1))
        .unwrap();
}

fn set(g: &mut Graph, id: u32, pin: &str, value: &str) {
    g.set_value(&End::new(id, pin), value.into());
}

/// A value node holding `text` as code.
fn lit(g: &mut Graph, text: &str) -> u32 {
    let id = add(g, "literal");
    set(g, id, "Text", text);
    id
}

/// `Start` running the statement `key`, pins typed or wired to a literal.
fn run(key: &str, values: &[(&str, &str)], wired: &[(&str, &str)]) -> String {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let node = add(&mut g, key);
    wire(&mut g, (start, ""), (node, ""));
    fill(&mut g, node, values, wired);
    compile(&g).unwrap()
}

/// `print(<value node>)`, the node filled like `run`.
fn show(key: &str, values: &[(&str, &str)], wired: &[(&str, &str)]) -> String {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let print = add(&mut g, "print");
    let node = add(&mut g, key);
    wire(&mut g, (start, ""), (print, ""));
    wire(&mut g, (node, "Value"), (print, "Value"));
    fill(&mut g, node, values, wired);
    compile(&g).unwrap()
}

fn fill(g: &mut Graph, node: u32, values: &[(&str, &str)], wired: &[(&str, &str)]) {
    for (pin, text) in values {
        set(g, node, pin, text);
    }
    for (pin, text) in wired {
        let source = lit(g, text);
        wire(g, (source, "Value"), (node, pin));
    }
}

#[test]
fn every_generic_statement_kind_compiles() {
    let cases = [
        (
            "local",
            run("local", &[("Value 1", "1")], &[]),
            "local x = 1\n",
        ),
        (
            "assign",
            run("assign", &[("Value 1", "2")], &[("Target 1", "a")]),
            "a = 2\n",
        ),
        (
            "compound",
            run("compound", &[("Op", "-=")], &[("Target", "n")]),
            "n -= 1\n",
        ),
        (
            "call",
            run("call", &[("Input 1", "1")], &[("Function", "f")]),
            "f(1)\n",
        ),
        (
            "method",
            run(
                "method",
                &[("Method", "Destroy"), ("#args", "0")],
                &[("Object", "part")],
            ),
            "part:Destroy()\n",
        ),
        (
            "if",
            run("if", &[("Condition 1", "true")], &[]),
            "if true then\nend\n",
        ),
        (
            "while",
            run("while", &[("Condition", "true")], &[]),
            "while true do\nend\n",
        ),
        (
            "repeat",
            run("repeat", &[("Condition", "true")], &[]),
            "repeat\nuntil true\n",
        ),
        (
            "for_count",
            run("for_count", &[("Step", "2")], &[]),
            "for i = 1, 10, 2 do\nend\n",
        ),
        (
            "for_each",
            run("for_each", &[], &[("In 1", "t")]),
            "for k, v in t do\nend\n",
        ),
        ("do", run("do", &[], &[]), "do\nend\n"),
        (
            "function",
            run("function", &[("Parameters", "a, b")], &[]),
            "function name(a, b)\nend\n",
        ),
        (
            "return",
            run("return", &[("Value 1", "7")], &[]),
            "return 7\n",
        ),
        ("break", run("break", &[], &[]), "break\n"),
        ("continue", run("continue", &[], &[]), "continue\n"),
        (
            "type",
            run("type", &[("Text", "type N = number")], &[]),
            "type N = number\n",
        ),
        (
            "comment",
            run("comment", &[("Text", "-- hi")], &[]),
            "-- hi\n",
        ),
    ];
    for (key, got, want) in cases {
        assert_eq!(got, want, "{key}");
    }
}

#[test]
fn every_generic_value_kind_compiles() {
    let cases = [
        (
            "get",
            show("get", &[("Name", "speed")], &[]),
            "print(speed)\n",
        ),
        (
            "field",
            show("field", &[("Name", "Position")], &[("Object", "part")]),
            "print(part.Position)\n",
        ),
        (
            "index",
            show("index", &[], &[("Object", "t"), ("Key", "1")]),
            "print(t[1])\n",
        ),
        (
            "call_value",
            show("call_value", &[("Input 1", "2")], &[("Function", "f")]),
            "print(f(2))\n",
        ),
        (
            "method_value",
            show(
                "method_value",
                &[("Method", "Clone"), ("#args", "0")],
                &[("Object", "p")],
            ),
            "print(p:Clone())\n",
        ),
        (
            "literal",
            show("literal", &[("Text", "42")], &[]),
            "print(42)\n",
        ),
        (
            "binary",
            show("binary", &[("A", "1"), ("Op", "*"), ("B", "2")], &[]),
            "print(1 * 2)\n",
        ),
        (
            "unary",
            show("unary", &[("Op", "-"), ("Value", "5")], &[]),
            "print(-5)\n",
        ),
        (
            "paren",
            show("paren", &[("Value", "5")], &[]),
            "print((5))\n",
        ),
        (
            "table",
            show("table", &[("Item 1", "5")], &[]),
            "print({5})\n",
        ),
        (
            "pair",
            show("pair", &[("Key", "a"), ("Value", "1")], &[]),
            "print([\"a\"] = 1)\n",
        ),
        (
            "if_value",
            show(
                "if_value",
                &[("Condition 1", "true"), ("Then 1", "1"), ("Else", "2")],
                &[],
            ),
            "print(if true then 1 else 2)\n",
        ),
        (
            "interp",
            show("interp", &[("Value 1", "5")], &[]),
            "print(`{5}`)\n",
        ),
        (
            "cast",
            show("cast", &[("Value", "5"), ("Type", "number")], &[]),
            "print(5 :: number)\n",
        ),
        (
            "function_value",
            show("function_value", &[("Parameters", "a")], &[]),
            "print(function(a)\nend)\n",
        ),
    ];
    for (key, got, want) in cases {
        assert_eq!(got, want, "{key}");
    }
}

/// `print(inner(a, b) <outer> c)`: the inner operator is the left operand.
fn left(outer: &str, inner: &str) -> String {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let print = add(&mut g, "print");
    let top = add(&mut g, "binary");
    let low = add(&mut g, "binary");
    wire(&mut g, (start, ""), (print, ""));
    wire(&mut g, (top, "Value"), (print, "Value"));
    wire(&mut g, (low, "Value"), (top, "A"));
    for (id, op) in [(top, outer), (low, inner)] {
        set(&mut g, id, "Op", op);
    }
    set(&mut g, low, "A", "1");
    set(&mut g, low, "B", "2");
    set(&mut g, top, "B", "3");
    compile(&g).unwrap()
}

/// `print(1 <outer> inner(b, 3))`: the inner operator is the right operand.
fn right(outer: &str, inner: &str) -> String {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let print = add(&mut g, "print");
    let top = add(&mut g, "binary");
    let low = add(&mut g, "binary");
    wire(&mut g, (start, ""), (print, ""));
    wire(&mut g, (top, "Value"), (print, "Value"));
    wire(&mut g, (low, "Value"), (top, "B"));
    for (id, op) in [(top, outer), (low, inner)] {
        set(&mut g, id, "Op", op);
    }
    set(&mut g, top, "A", "1");
    set(&mut g, low, "A", "2");
    set(&mut g, low, "B", "3");
    compile(&g).unwrap()
}

#[test]
fn operands_are_bracketed_by_precedence() {
    assert_eq!(left("*", "+"), "print((1 + 2) * 3)\n");
    assert_eq!(left("+", "*"), "print(1 * 2 + 3)\n");
    assert_eq!(left("and", "or"), "print((1 or 2) and 3)\n");
    assert_eq!(left("==", "+"), "print(1 + 2 == 3)\n");
}

#[test]
fn associativity_decides_which_side_brackets() {
    // Left-associative: the left operand goes bare, the right brackets.
    assert_eq!(left("-", "-"), "print(1 - 2 - 3)\n");
    assert_eq!(right("-", "-"), "print(1 - (2 - 3))\n");
    // Right-associative: the other way round.
    assert_eq!(left("^", "^"), "print((1 ^ 2) ^ 3)\n");
    assert_eq!(right("^", "^"), "print(1 ^ 2 ^ 3)\n");
    assert_eq!(left("..", ".."), "print((1 .. 2) .. 3)\n");
    assert_eq!(right("..", ".."), "print(1 .. 2 .. 3)\n");
}

#[test]
fn a_unary_minus_binds_looser_than_a_power() {
    let build = |minus_outside: bool| {
        let mut g = Graph::default();
        let start = add(&mut g, "start");
        let print = add(&mut g, "print");
        let minus = add(&mut g, "unary");
        let power = add(&mut g, "binary");
        wire(&mut g, (start, ""), (print, ""));
        set(&mut g, minus, "Op", "-");
        set(&mut g, power, "Op", "^");
        if minus_outside {
            wire(&mut g, (minus, "Value"), (print, "Value"));
            wire(&mut g, (power, "Value"), (minus, "Value"));
            set(&mut g, power, "A", "5");
            set(&mut g, power, "B", "2");
        } else {
            wire(&mut g, (power, "Value"), (print, "Value"));
            wire(&mut g, (minus, "Value"), (power, "A"));
            set(&mut g, minus, "Value", "5");
            set(&mut g, power, "B", "2");
        }
        compile(&g).unwrap()
    };
    assert_eq!(build(true), "print(-5 ^ 2)\n");
    assert_eq!(build(false), "print((-5) ^ 2)\n");
}

#[test]
fn a_value_read_twice_is_hoisted_and_read_once_is_inline() {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let a = add(&mut g, "print");
    let b = add(&mut g, "print");
    let name = add(&mut g, "get_name");
    wire(&mut g, (start, ""), (a, ""));
    wire(&mut g, (a, ""), (b, ""));
    wire(&mut g, (name, "Name"), (a, "Value"));
    set(&mut g, name, "Instance", "workspace");
    let once = compile(&g).unwrap();
    assert!(!once.contains("local"), "{once}");
    wire(&mut g, (name, "Name"), (b, "Value"));
    let twice = compile(&g).unwrap();
    assert!(twice.starts_with("local "), "{twice}");
    assert_eq!(twice.matches("workspace.Name").count(), 1, "{twice}");
}

fn two_prints() -> (Graph, u32, u32) {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let first = add(&mut g, "print");
    let second = add(&mut g, "print");
    wire(&mut g, (start, ""), (first, ""));
    wire(&mut g, (first, ""), (second, ""));
    set(&mut g, first, "Value", "\"one\"");
    set(&mut g, second, "Value", "\"two\"");
    (g, first, second)
}

fn origin(g: &Graph, id: u32, lead: &str, text: &str) -> Origin {
    Origin {
        lead: lead.into(),
        text: text.into(),
        print: fingerprint(g, id),
        ..Default::default()
    }
}

#[test]
fn a_clean_statement_keeps_its_text_and_an_edited_one_is_rewritten() {
    let (mut g, first, second) = two_prints();
    let mut origins = Origins::default();
    origins
        .stmts
        .insert(first, origin(&g, first, "-- keep\n", "print(  \"one\" )"));
    origins
        .stmts
        .insert(second, origin(&g, second, "", "print( \"two\" )"));
    origins.tail = "-- end\n".into();
    let out = compile_with(&g, &origins).unwrap();
    assert!(out.contains("-- keep\n"), "{out}");
    assert!(out.contains("print(  \"one\" )"), "{out}");
    assert!(out.contains("print( \"two\" )"), "{out}");
    assert!(out.ends_with("-- end\n"), "{out}");

    set(&mut g, second, "Value", "\"three\"");
    let out = compile_with(&g, &origins).unwrap();
    assert!(out.contains("print(  \"one\" )"), "{out}");
    assert!(out.contains("print(\"three\")"), "{out}");
    assert!(!out.contains("two"), "{out}");
}

#[test]
fn a_fingerprint_ignores_id_and_position_and_follows_values() {
    let (mut g, first, _) = two_prints();
    let before = fingerprint(&g, first);
    // The same statement in another graph: other ids, far away.
    let mut h = Graph::default();
    let start = h.add(catalog::kind("start").unwrap(), [300.0, 40.0]);
    let _pad = add(&mut h, "print");
    let moved = h.add(catalog::kind("print").unwrap(), [500.0, 500.0]);
    wire(&mut h, (start, ""), (moved, ""));
    set(&mut h, moved, "Value", "\"one\"");
    assert_eq!(fingerprint(&h, moved), before);
    set(&mut g, first, "Value", "\"uno\"");
    assert_ne!(fingerprint(&g, first), before);
}

#[test]
fn anchors_number_identical_nodes_in_order() {
    let mut g = Graph::default();
    let start = add(&mut g, "start");
    let a = add(&mut g, "print");
    let b = add(&mut g, "print");
    let c = add(&mut g, "print");
    wire(&mut g, (start, ""), (a, ""));
    wire(&mut g, (a, ""), (b, ""));
    wire(&mut g, (b, ""), (c, ""));
    set(&mut g, c, "Value", "\"other\"");
    let keys = anchors(&g);
    assert!(keys[&a].ends_with("#0"), "{}", keys[&a]);
    assert!(keys[&b].ends_with("#1"), "{}", keys[&b]);
    assert!(keys[&c].ends_with("#0"), "{}", keys[&c]);
}
