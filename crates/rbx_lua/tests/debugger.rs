//! The debugger end to end: breakpoints of every kind, stepping, and what a
//! paused script exposes.

use std::cell::RefCell;
use std::rc::Rc;

use rbx_dom::{Instance, Ref, WeakDom};
use rbx_lua::{Breakpoint, Frame, Paused, Resume, Runtime, Variable, STOPPED};
use rbx_reflection::ReflectionDatabase;

fn runtime() -> Runtime {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

fn at(line: u32) -> Breakpoint {
    Breakpoint {
        line,
        ..Breakpoint::default()
    }
}

/// Runs `source`, answering every pause from `answers` in turn (then
/// `Continue`), and returns what `inspect` read at each pause and the output.
fn debug<T: 'static>(
    source: &str,
    breakpoints: &[Breakpoint],
    answers: &[Resume],
    inspect: impl Fn(&Paused) -> T + 'static,
) -> (Vec<T>, Result<Vec<String>, String>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let answers = RefCell::new(answers.to_vec());
    let recorder = seen.clone();
    let result = runtime()
        .debug(source, "Script", breakpoints, move |paused| {
            recorder.borrow_mut().push(inspect(paused));
            let mut answers = answers.borrow_mut();
            if answers.is_empty() {
                Resume::Continue
            } else {
                answers.remove(0)
            }
        })
        .map(|output| output.lines().to_vec())
        .map_err(|err| err.to_string());
    let seen = Rc::try_unwrap(seen)
        .ok()
        .expect("session ended")
        .into_inner();
    (seen, result)
}

const LOOP: &str = "local total = 0
for i = 1, 3 do
    total += i
end
print(total)";

#[test]
fn a_standard_breakpoint_pauses_every_time_its_line_is_reached() {
    let (lines, result) = debug(LOOP, &[at(3)], &[], |p| p.line());
    assert_eq!(lines, vec![3, 3, 3]);
    assert_eq!(result.unwrap(), vec!["6"]);
}

#[test]
fn a_conditional_breakpoint_only_pauses_while_its_condition_holds() {
    let breakpoint = Breakpoint {
        condition: Some("i == 2".into()),
        ..at(3)
    };
    let (seen, _) = debug(LOOP, &[breakpoint], &[], |p| p.evaluate("i"));
    assert_eq!(seen, vec![Ok("2".to_owned())]);
}

#[test]
fn a_logpoint_prints_without_pausing() {
    let breakpoint = Breakpoint {
        log_message: Some(r#""i is", i, "total is", total"#.into()),
        continue_execution: true,
        ..at(3)
    };
    let (pauses, result) = debug(LOOP, &[breakpoint], &[], |p| p.line());
    assert!(pauses.is_empty());
    assert_eq!(
        result.unwrap(),
        vec![
            "i is 1 total is 0",
            "i is 2 total is 1",
            "i is 3 total is 3",
            "6"
        ]
    );
}

#[test]
fn a_failing_condition_pauses_and_says_why() {
    let breakpoint = Breakpoint {
        condition: Some("nope.field".into()),
        ..at(1)
    };
    let (pauses, result) = debug("print(1)", &[breakpoint], &[], |p| p.line());
    assert_eq!(pauses, vec![1]);
    let output = result.unwrap();
    assert!(output[0].starts_with("Breakpoint condition `nope.field` failed"));
    assert_eq!(output[1], "1");
}

const CALLS: &str = "local function add(a, b)
    local sum = a + b
    return sum
end
local x = add(1, 2)
local y = add(x, 3)
print(y)";

#[test]
fn step_over_stays_in_the_function_it_started_in() {
    let (lines, _) = debug(
        CALLS,
        &[at(5)],
        &[Resume::StepOver, Resume::StepOver],
        |p| p.line(),
    );
    assert_eq!(lines, vec![5, 6, 7]);
}

#[test]
fn step_into_enters_the_called_function_and_step_out_leaves_it() {
    let (lines, _) = debug(
        CALLS,
        &[at(5)],
        &[Resume::StepInto, Resume::StepOut, Resume::StepOver],
        |p| p.line(),
    );
    // Into lands on `add`'s first line; Out finishes the calling line too,
    // as Studio's Step Out does, landing on the line after the call.
    assert_eq!(lines, vec![5, 2, 6, 7]);
}

#[test]
fn the_call_stack_lists_innermost_first() {
    let (stacks, _) = debug(CALLS, &[at(3)], &[], |p| p.call_stack());
    assert_eq!(
        stacks[0],
        vec![
            Frame {
                function: "add".into(),
                line: 3
            },
            Frame {
                function: "main chunk".into(),
                line: 5
            },
        ]
    );
}

#[test]
fn variables_show_the_innermost_scope_formatted() {
    let source = "local label = \"hi\"
local function show(list)
    local count = #list
    return count
end
show({1, 2})";
    let (seen, _) = debug(source, &[at(4)], &[], |p| p.variables());
    let names: Vec<Variable> = seen.into_iter().next().unwrap();
    assert_eq!(
        names,
        vec![
            Variable {
                name: "list".into(),
                value: "{[1] = 1, [2] = 2}".into()
            },
            Variable {
                name: "count".into(),
                value: "2".into()
            },
        ]
    );
}

#[test]
fn watches_see_upvalues_and_globals() {
    let source = "local base = 10
local function f()
    return base + 1
end
f()";
    let (seen, _) = debug(source, &[at(3)], &[], |p| {
        (p.evaluate("base * 2"), p.evaluate("workspace.Name"))
    });
    assert_eq!(
        seen,
        vec![(Ok("20".to_owned()), Ok("\"Workspace\"".to_owned()))]
    );
}

#[test]
fn stop_abandons_the_script_but_keeps_what_it_already_did() {
    let source = r#"Instance.new("Folder", workspace).Name = "Kept"
print("never")"#;
    let mut runtime = runtime();
    let result = runtime.debug(source, "Script", &[at(2)], |_| Resume::Stop);
    assert!(result.unwrap_err().to_string().contains(STOPPED));
    let dom = runtime.into_dom();
    let workspace = dom.get(Ref::new(1)).unwrap();
    assert_eq!(dom.get(workspace.children()[0]).unwrap().name(), "Kept");
}

#[test]
fn a_runtime_is_reusable_after_a_debug_run() {
    let mut runtime = runtime();
    runtime
        .debug("print(1)", "Script", &[at(1)], |_| Resume::Continue)
        .unwrap();
    let output = runtime.run("print(2)").unwrap();
    assert_eq!(output.lines(), ["2"]);
}

#[test]
fn output_printed_before_a_pause_can_be_taken_while_paused() {
    let (seen, result) = debug("print(\"a\")\nprint(\"b\")", &[at(2)], &[], |p| {
        p.take_output()
    });
    assert_eq!(seen, vec![vec!["a".to_owned()]]);
    assert_eq!(result.unwrap(), vec!["b"]);
}
