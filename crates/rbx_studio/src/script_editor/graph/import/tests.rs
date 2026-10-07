use super::super::codegen::compile;
use super::*;

fn raw_nodes(graph: &Graph) -> usize {
    graph
        .nodes
        .iter()
        .filter(|node| node.kind == "luau")
        .count()
}

/// Imports `src` and checks the graph compiles back to the same code.
fn lossless(src: &str) -> (Graph, String) {
    let graph = import(src);
    let out = compile(&graph).unwrap_or_else(|problems| panic!("{src}\n{problems:?}"));
    let lexed = lex::lex(src).expect("test sources lex");
    assert_eq!(verify(&lexed, src, &out), Ok(()), "\n{src}\n---\n{out}");
    (graph, out)
}

/// Lossless, drawn wholly as nodes, and stable when read again.
fn converts(src: &str) -> String {
    let (graph, out) = lossless(src);
    assert_eq!(raw_nodes(&graph), 0, "kept raw:\n{src}\n---\n{out}");
    assert_eq!(compile(&import(&out)).ok().as_deref(), Some(out.as_str()));
    out
}

#[test]
fn a_call_round_trips_exactly() {
    assert_eq!(converts("print(\"Hello\")").trim_end(), "print(\"Hello\")");
}

#[test]
fn an_event_with_a_lookup_and_a_branch_converts() {
    let src = "script.Parent.Touched:Connect(function(hit)
\tlocal humanoid = hit.Parent:FindFirstChild(\"Humanoid\")
\tif humanoid ~= nil then
\t\thumanoid:Destroy()
\tend
end)";
    let out = converts(src);
    assert!(out.contains("Touched:Connect(function("), "{out}");
}

#[test]
fn loops_convert() {
    converts("for i = 1, 3 do\n\tprint(i)\nend");
    converts("local children = workspace:GetChildren()\nfor index, child in children do\n\tprint(child)\nend");
}

#[test]
fn a_branch_with_else_carries_on_after_it() {
    converts("if workspace.Gravity > 100 then\n\tprint(\"heavy\")\nelse\n\tprint(\"light\")\nend\nprint(\"done\")");
}

#[test]
fn precedence_is_kept() {
    converts("print(1 + 2 * 3)");
    converts("print((1 + 2) * 3)");
    // The compiler would bracket the left side; kept as written instead.
    let (graph, out) = lossless("print(1 - 2 - 3)");
    assert_eq!(raw_nodes(&graph), 1);
    assert_eq!(out.trim_end(), "print(1 - 2 - 3)");
}

#[test]
fn what_cannot_be_drawn_is_kept_as_written() {
    let src = "-- greet someone
local function greet(name)
\tprint(\"hi \" .. name)
end

greet(\"x\") -- trailing
local s = [[
  keep
\tthis]]
print(s)
while true do
\ttask.wait(1)
end";
    let (graph, out) = lossless(src);
    assert!(raw_nodes(&graph) >= 1);
    assert!(out.contains("[[\n  keep\n\tthis]]"), "{out}");
}

#[test]
fn an_event_body_mixes_nodes_and_code() {
    let src = "print(1)
script.Parent.Touched:Connect(function(hit)
\tprint(\"touched\")
\tlocal t = {1, 2}
\tprint(#t)
end)
print(2)";
    let (graph, _) = lossless(src);
    assert!(graph.nodes.iter().any(|node| node.kind == "touched"));
    assert!(raw_nodes(&graph) >= 1);
}

#[test]
fn broken_code_is_one_luau_node() {
    for src in ["print(\"unterminated", "if x then\n\tprint(1)", "local = 3"] {
        let graph = import(src);
        assert_eq!(raw_nodes(&graph), 1, "{src}");
        assert_eq!(compile(&graph).unwrap().trim_end(), src);
    }
}

#[test]
fn arbitrary_scripts_are_lossless() {
    for src in [
        "local Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(function(player)\n\tprint(player.Name)\nend)",
        "local a, b = 1, 2\na, b = b, a\nprint(a; b)",
        "local t = {1, 2, 3}\nfor _, v in ipairs(t) do\n\tif v > 1 then\n\t\tprint(v)\n\telseif v == 1 then\n\t\twarn(v)\n\tend\nend",
        "repeat\n\tlocal x = math.random()\nuntil x > 0.5\nreturn nil",
        "print(`hi {script.Name}`)\n--[[ long\ncomment ]]\nprint('single')",
        "local x = 5\nprint(x)\nprint(x + 1)",
    ] {
        lossless(src);
    }
}

#[test]
fn empty_code_is_an_empty_graph() {
    assert_eq!(import("  \n"), Graph::default());
}

#[test]
fn raw_lines_keep_multiline_strings() {
    assert_eq!(
        raw_lines("a = [[x\n  y]]\r\nb()\r"),
        vec![("a = [[x", false), ("  y]]\r", true), ("b()", false)]
    );
    assert!(raw_lines("").is_empty());
}

#[test]
fn code_reading_an_event_parameter_keeps_the_event_as_written() {
    // Generated parameters are named by the node, never as raw code
    // spells them, so the whole event stays Luau rather than break.
    let src = "script.Parent.Touched:Connect(function(hit)\n\tlocal t = {hit}\n\tprint(#t)\nend)";
    let (graph, out) = lossless(src);
    assert_eq!(raw_nodes(&graph), 1);
    assert_eq!(out.trim_end(), src);
}
