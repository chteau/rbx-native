use super::super::codegen;
use super::import;

fn round(src: &str) -> super::Imported {
    let got = import(src);
    assert!(got.broken.is_none(), "{:?}", got.broken.map(|b| b.message));
    assert_eq!(codegen::compile_with(&got.graph, &got.origins).as_deref(), Ok(src));
    got
}

fn kinds(src: &str) -> Vec<String> {
    round(src).graph.nodes.iter().map(|n| n.kind.clone()).collect()
}

#[test]
fn catalog_print_is_recognised() {
    assert!(kinds("print(\"hi\")\n").iter().any(|k| k == "print"));
}

#[test]
fn event_params_become_outputs() {
    let got = round("workspace.ChildAdded:Connect(function(child)\n\tprint(child)\nend)\n");
    let event = got
        .graph
        .nodes
        .iter()
        .find(|n| n.values.keys().any(|k| k.starts_with("@name:")))
        .expect("event node");
    assert_eq!(event.values.values().next().map(String::as_str), Some("child"));
}

#[test]
fn local_read_twice_is_folded() {
    let folded = round("local n = foo()\nprint(n, n)\n");
    assert!(!folded.graph.nodes.iter().any(|n| n.kind == "local"));
    let kept = round("local n = foo()\nprint(n)\n");
    assert!(kept.graph.nodes.iter().any(|n| n.kind == "local"));
}

#[test]
fn comments_keep_their_place() {
    round("-- top\nlocal a = 1 -- after\n\n-- own line\nprint(a)\n");
    round("local a = 1 + --[[mid]] 2\nprint(a)\n");
}

#[test]
fn syntax_error_is_broken_with_its_line() {
    let got = import("print(1)\nlocal = \n");
    let broken = got.broken.expect("broken");
    assert_eq!(broken.line, 2);
    assert!(got.graph.nodes.iter().any(|n| n.kind == "luau"));
    assert!(got.graph.nodes.iter().any(|n| n.kind == "start"));
}

#[test]
fn same_ignores_nothing_but_whitespace() {
    assert!(super::same("a=1", "a = 1"));
    assert!(!super::same("a=1", "a = 2"));
}
