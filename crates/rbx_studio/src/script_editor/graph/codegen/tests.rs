use super::super::catalog::{self, PinType, Prec};
use super::super::{End, Graph};
use super::{compile, literal, quote};

fn add(graph: &mut Graph, key: &str, at: [f32; 2]) -> u32 {
    graph.add(catalog::kind(key).unwrap(), at)
}

fn wire(graph: &mut Graph, from: (u32, &str), to: (u32, &str)) {
    graph
        .connect(End::new(from.0, from.1), End::new(to.0, to.1))
        .unwrap();
}

/// The Damage on touch board.
fn damage_on_touch() -> Graph {
    let mut g = Graph::default();
    let touched = add(&mut g, "touched", [0.0, 0.0]);
    let parent = add(&mut g, "get_parent", [0.0, 0.0]);
    let find = add(&mut g, "find_child_of_class", [0.0, 0.0]);
    let valid = add(&mut g, "is_valid", [0.0, 0.0]);
    let branch = add(&mut g, "branch", [0.0, 0.0]);
    let get = add(&mut g, "get_property", [0.0, 0.0]);
    let subtract = add(&mut g, "subtract", [0.0, 0.0]);
    let set = add(&mut g, "set_property", [0.0, 0.0]);
    wire(&mut g, (touched, ""), (branch, ""));
    wire(&mut g, (touched, "hit"), (parent, "Instance"));
    wire(&mut g, (parent, "Parent"), (find, "Parent"));
    wire(&mut g, (find, "Result"), (valid, "Value"));
    wire(&mut g, (valid, "Result"), (branch, "Condition"));
    wire(&mut g, (branch, "True"), (set, ""));
    wire(&mut g, (find, "Result"), (set, "Target"));
    wire(&mut g, (find, "Result"), (get, "Target"));
    wire(&mut g, (get, "Value"), (subtract, "A"));
    wire(&mut g, (subtract, "Result"), (set, "Value"));
    g.set_value(&End::new(get, "Property"), "Health".into());
    g.set_value(&End::new(set, "Property"), "Health".into());
    g.set_value(&End::new(subtract, "B"), "25".into());
    g
}

#[test]
fn the_damage_board_compiles_to_the_script_it_draws() {
    assert_eq!(
        compile(&damage_on_touch()).unwrap(),
        "script.Parent.Touched:Connect(function(hit)\n\
         \tlocal humanoid = hit.Parent:FindFirstChildOfClass(\"Humanoid\")\n\
         \tif humanoid ~= nil then\n\
         \t\thumanoid.Health = humanoid.Health - 25\n\
         \tend\n\
         end)\n"
    );
}

#[test]
fn an_empty_graph_is_an_empty_script() {
    assert_eq!(compile(&Graph::default()).unwrap(), "");
}

#[test]
fn on_start_runs_at_the_top_level_and_events_follow_in_reading_order() {
    let mut g = Graph::default();
    let late = add(&mut g, "player_added", [0.0, 300.0]);
    let start = add(&mut g, "start", [0.0, 0.0]);
    let hello = add(&mut g, "print", [0.0, 0.0]);
    let name = add(&mut g, "get_name", [0.0, 0.0]);
    let greet = add(&mut g, "print", [0.0, 0.0]);
    wire(&mut g, (start, ""), (hello, ""));
    wire(&mut g, (late, ""), (greet, ""));
    wire(&mut g, (late, "player"), (name, "Instance"));
    wire(&mut g, (name, "Name"), (greet, "Value"));
    assert_eq!(
        compile(&g).unwrap(),
        "print(\"Hello\")\n\
         \n\
         game:GetService(\"Players\").PlayerAdded:Connect(function(player)\n\
         \tprint(player.Name)\n\
         end)\n"
    );
}

#[test]
fn a_branch_with_only_false_wired_negates_its_condition() {
    let mut g = Graph::default();
    let start = add(&mut g, "start", [0.0, 0.0]);
    let branch = add(&mut g, "branch", [0.0, 0.0]);
    let equal = add(&mut g, "equal", [0.0, 0.0]);
    let print = add(&mut g, "print", [0.0, 0.0]);
    wire(&mut g, (start, ""), (branch, ""));
    wire(&mut g, (equal, "Result"), (branch, "Condition"));
    wire(&mut g, (branch, "False"), (print, ""));
    g.set_value(&End::new(equal, "A"), "1".into());
    g.set_value(&End::new(equal, "B"), "2".into());
    assert_eq!(
        compile(&g).unwrap(),
        "if not (1 == 2) then\n\tprint(\"Hello\")\nend\n"
    );
}

#[test]
fn loops_name_their_outputs_and_carry_on_when_done() {
    let mut g = Graph::default();
    let start = add(&mut g, "start", [0.0, 0.0]);
    let children = add(&mut g, "get_children", [0.0, 0.0]);
    let each = add(&mut g, "for_each", [0.0, 0.0]);
    let destroy = add(&mut g, "destroy", [0.0, 0.0]);
    let done = add(&mut g, "print", [0.0, 0.0]);
    let path = add(&mut g, "instance", [0.0, 0.0]);
    wire(&mut g, (path, "Instance"), (children, "Instance"));
    wire(&mut g, (start, ""), (each, ""));
    wire(&mut g, (children, "Children"), (each, "List"));
    wire(&mut g, (each, "Loop"), (destroy, ""));
    wire(&mut g, (each, "Item"), (destroy, "Instance"));
    wire(&mut g, (each, "Completed"), (done, ""));
    assert_eq!(
        compile(&g).unwrap(),
        "local children = workspace:GetChildren()\n\
         for index, item in children do\n\
         \titem:Destroy()\n\
         end\n\
         print(\"Hello\")\n"
    );
}

#[test]
fn operands_are_wrapped_only_where_precedence_needs_it() {
    let mut g = Graph::default();
    let start = add(&mut g, "start", [0.0, 0.0]);
    let print = add(&mut g, "print", [0.0, 0.0]);
    let sum = add(&mut g, "add", [0.0, 0.0]);
    let outer = add(&mut g, "multiply", [0.0, 0.0]);
    let inner = add(&mut g, "multiply", [0.0, 0.0]);
    wire(&mut g, (start, ""), (print, ""));
    wire(&mut g, (sum, "Result"), (outer, "A"));
    wire(&mut g, (inner, "Result"), (sum, "B"));
    wire(&mut g, (outer, "Result"), (print, "Value"));
    assert_eq!(compile(&g).unwrap(), "print((0 + 0 * 0) * 0)\n");
}

#[test]
fn a_value_used_outside_its_event_is_a_problem() {
    let mut g = Graph::default();
    let touched = add(&mut g, "touched", [0.0, 0.0]);
    let start = add(&mut g, "start", [0.0, 100.0]);
    let destroy = add(&mut g, "destroy", [0.0, 0.0]);
    wire(&mut g, (start, ""), (destroy, ""));
    wire(&mut g, (touched, "hit"), (destroy, "Instance"));
    let problems = compile(&g).unwrap_err();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].node, touched);
}

#[test]
fn an_unwired_required_input_is_a_problem_on_its_node() {
    let mut g = Graph::default();
    let start = add(&mut g, "start", [0.0, 0.0]);
    let destroy = add(&mut g, "destroy", [0.0, 0.0]);
    wire(&mut g, (start, ""), (destroy, ""));
    let problems = compile(&g).unwrap_err();
    assert_eq!(problems[0].node, destroy);
    assert_eq!(problems[0].message, "Destroy needs its Instance wired");
}

#[test]
fn a_run_looping_into_itself_is_a_problem() {
    let mut g = Graph::default();
    let start = add(&mut g, "start", [0.0, 0.0]);
    let a = add(&mut g, "wait", [0.0, 0.0]);
    let b = add(&mut g, "wait", [0.0, 0.0]);
    wire(&mut g, (start, ""), (a, ""));
    wire(&mut g, (a, ""), (b, ""));
    wire(&mut g, (b, ""), (a, ""));
    assert!(compile(&g).is_err());
}

#[test]
fn literals_are_checked_and_quoted() {
    assert_eq!(
        literal("25", PinType::Number),
        Ok(("25".into(), Prec::Atom))
    );
    assert_eq!(
        literal("-2", PinType::Number),
        Ok(("-2".into(), Prec::Unary))
    );
    assert!(literal("ten", PinType::Number).is_err());
    assert!(literal("yes", PinType::Bool).is_err());
    assert_eq!(
        literal("workspace.Lava", PinType::Instance).unwrap().0,
        "workspace.Lava"
    );
    assert!(literal("os.exit()", PinType::Instance).is_err());
    assert_eq!(literal("hi", PinType::Any).unwrap().0, "\"hi\"");
    assert_eq!(literal("\"hi\"", PinType::Any).unwrap().0, "\"hi\"");
    assert_eq!(literal("nil", PinType::Any).unwrap().0, "nil");
    assert_eq!(quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
}

#[test]
fn an_unknown_kind_is_a_problem_not_a_panic() {
    let mut g = Graph::default();
    let id = add(&mut g, "print", [0.0, 0.0]);
    g.node_mut(id).unwrap().kind = "from_the_future".into();
    assert_eq!(compile(&g).unwrap_err()[0].node, id);
}
