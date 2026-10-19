use std::cell::Cell;

use rbx_dom::Variant;

use super::*;

fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    dom
}

fn runtime() -> Runtime {
    Runtime::new(place(), ReflectionDatabase::embedded()).unwrap()
}

/// A runtime whose clock only moves when the test says so.
fn with_fake_clock() -> (Runtime, Rc<Cell<f64>>) {
    let mut runtime = runtime();
    let clock = Rc::new(Cell::new(0.0));
    let read = clock.clone();
    runtime.set_clock(move || Duration::from_secs_f64(read.get()));
    (runtime, clock)
}

fn lines(source: &str) -> Vec<String> {
    match runtime().run(source) {
        Ok(output) => output.lines().to_vec(),
        Err(error) => panic!("{error} after {:?}", error.output.lines()),
    }
}

fn wake_in(step: &Step) -> f64 {
    match step.state {
        RunState::Pending { wake_in } => wake_in.as_secs_f64(),
        ref other => panic!("expected a pending run, got {other:?}"),
    }
}

#[test]
fn a_script_that_never_ends_times_out() {
    let mut runtime = runtime();
    runtime.budget = Duration::from_millis(100);

    let error = runtime.run("while true do end").expect_err("it must stop");

    assert!(error.to_string().contains("Script timeout"), "{error}");
    assert!(runtime.run("print(1)").is_ok(), "the runtime stays usable");
}

#[test]
fn a_run_yields_and_resumes_on_the_host_clock() {
    let (mut runtime, clock) = with_fake_clock();
    runtime.start("print('a') local slept = task.wait(1) print('b', slept >= 1)");

    let first = runtime.resume();
    assert_eq!(first.output.lines(), ["a"]);
    assert!((wake_in(&first) - 1.0).abs() < 1e-9);

    clock.set(0.5);
    let early = runtime.resume();
    assert!(early.output.is_empty());
    assert!((wake_in(&early) - 0.5).abs() < 1e-9);

    clock.set(1.0);
    let last = runtime.resume();
    assert_eq!(last.output.lines(), ["b true"]);
    assert!(matches!(last.state, RunState::Finished));
}

#[test]
fn each_resume_gets_a_fresh_budget() {
    let mut runtime = runtime();
    runtime.budget = Duration::from_millis(300);
    let busy = "local t = os.clock() while os.clock() - t < 0.2 do end";
    let output = runtime
        .run(&format!(
            "{busy} task.wait() {busy} task.wait() {busy} print('done')"
        ))
        .expect("no single resume runs past the budget");
    assert_eq!(output.lines(), ["done"]);
}

#[test]
fn output_before_an_error_survives_everywhere() {
    let error = runtime()
        .run("print('before') warn('careful') error('boom')")
        .expect_err("the script fails");
    assert_eq!(error.output.lines(), ["before", "careful"]);
    assert_eq!(error.output.kinds(), [LineKind::Print, LineKind::Warn]);
    assert!(error.to_string().contains("Command bar:1: boom"), "{error}");

    let mut stepped = runtime();
    stepped.start("print('one') task.wait() print('two') error('late')");
    assert_eq!(stepped.resume().output.lines(), ["one"]);
    let failed = stepped.resume();
    assert_eq!(failed.output.lines(), ["two"]);
    assert!(matches!(failed.state, RunState::Failed(_)));

    let stop = Arc::new(AtomicBool::new(false));
    let debugged = runtime()
        .debug("print('kept')\nerror('x')", "Script", &[], stop, |_| {
            Resume::Continue
        })
        .expect_err("the script fails");
    assert_eq!(debugged.output.lines(), ["kept"]);
}

#[test]
fn a_chunk_that_does_not_compile_fails_its_first_resume() {
    let mut runtime = runtime();
    runtime.start("local = 1");
    assert!(matches!(runtime.resume().state, RunState::Failed(_)));
}

#[test]
fn errors_in_spawned_threads_are_output_not_failures() {
    let output = runtime()
        .run("task.spawn(function() error('inner') end) print('after')")
        .expect("the main chunk survives");
    assert_eq!(output.kinds(), [LineKind::Error, LineKind::Print]);
    assert!(output.lines()[0].contains("inner"), "{:?}", output.lines());
    assert_eq!(output.lines()[1], "after");
}

#[test]
fn task_library_and_time_globals() {
    let printed = lines(
        "task.delay(0.01, function(x) print('delayed', x) end, 3) \
         local c = task.delay(0.01, function() print('cancelled') end) task.cancel(c) \
         task.defer(function() print('deferred') end) \
         spawn(function() print('spawned') end) \
         delay(0, function() print('legacy delay') end) \
         wait(0.03) \
         print(type(tick()), type(time()), type(os.clock()))",
    );
    for expected in [
        "spawned",
        "deferred",
        "delayed 3",
        "legacy delay",
        "number number number",
    ] {
        assert!(
            printed.iter().any(|l| l == expected),
            "{expected}: {printed:?}"
        );
    }
    assert!(!printed.iter().any(|l| l == "cancelled"), "{printed:?}");
}

#[test]
fn cancel_drops_a_pending_run() {
    let (mut runtime, clock) = with_fake_clock();
    runtime.start("task.wait(1) print('late')");
    assert!(matches!(runtime.resume().state, RunState::Pending { .. }));
    runtime.cancel();
    clock.set(2.0);
    let step = runtime.resume();
    assert!(step.output.is_empty());
    assert!(matches!(step.state, RunState::Finished));
}

#[test]
fn the_host_lends_its_dom_for_each_resume() {
    let mut dom = place();
    let mut runtime = Runtime::lent(&mut dom, ReflectionDatabase::embedded()).unwrap();
    let clock = Rc::new(Cell::new(0.0));
    let read = clock.clone();
    runtime.set_clock(move || Duration::from_secs_f64(read.get()));
    runtime.start(
        "local f = Instance.new('Folder', workspace) f.Name = 'F' task.wait(1) f.Name = 'G'",
    );

    runtime.resume_in(&mut dom);
    let workspace = dom.root_refs()[0];
    let folder = dom.get(workspace).unwrap().children()[0];
    assert_eq!(dom.get(folder).unwrap().name(), "F");
    assert!(
        !dom.take_changes().is_empty(),
        "the resume left the log alone"
    );

    clock.set(1.0);
    assert!(matches!(
        runtime.resume_in(&mut dom).state,
        RunState::Finished
    ));
    assert_eq!(dom.get(folder).unwrap().name(), "G", "same tree, same Ref");
}

#[test]
fn run_to_completion_drives_a_wait_then_a_dom_write() {
    let mut runtime = runtime();
    runtime
        .run("task.wait(0.05) workspace.Name = 'Waited'")
        .expect("the run finishes");
    let dom = runtime.dom();
    let workspace = dom.root_refs()[0];
    assert_eq!(dom.get(workspace).unwrap().name(), "Waited");
    assert!(
        !dom.changes().is_empty(),
        "the change log is the host's to drain"
    );
}

#[test]
fn run_to_completion_abandons_threads_past_its_limit() {
    let output = runtime()
        .run_for("task.wait(60) print('never')", Duration::from_millis(50))
        .expect("abandoning is not a failure");
    assert_eq!(output.kinds(), [LineKind::Warn]);
    assert!(
        output.lines()[0].contains("abandoned"),
        "{:?}",
        output.lines()
    );
}

#[test]
fn attributes_and_their_signals() {
    let printed = lines(
        "local f = Instance.new('Folder') \
         f:GetAttributeChangedSignal('Speed'):Connect(function() print('speed', f:GetAttribute('Speed')) end) \
         f.AttributeChanged:Connect(function(name) print('attribute', name) end) \
         f:SetAttribute('Speed', 5) \
         print(f:GetAttributes().Speed, f:GetAttribute('Missing'))",
    );
    assert_eq!(printed[0], "5 nil");
    assert!(printed.contains(&"speed 5".to_owned()), "{printed:?}");
    assert!(
        printed.contains(&"attribute Speed".to_owned()),
        "{printed:?}"
    );
}

#[test]
fn changed_reaches_only_connections_made_before_the_change() {
    let printed = lines(
        "local p = Instance.new('Part', workspace) \
         p.Changed:Connect(function(name) print('changed', name) end) \
         p:GetPropertyChangedSignal('Name'):Connect(function() print('renamed', p.Name) end) \
         p.Name = 'X'",
    );
    assert_eq!(printed, ["changed Name", "renamed X"]);
}

#[test]
fn bindable_events_and_functions() {
    let printed = lines(
        "local e = Instance.new('BindableEvent') \
         e.Event:Connect(function(a, b) print('fired', a + b) end) \
         task.spawn(function() print('waited', e.Event:Wait()) end) \
         e:Fire(2, 5) \
         local f = Instance.new('BindableFunction') \
         f.OnInvoke = function(x) return x * 2 end \
         print('invoked', f:Invoke(21))",
    );
    assert_eq!(printed, ["fired 7", "waited 2 5", "invoked 42"]);
}

#[test]
fn collection_service_takes_the_instance_first() {
    let printed = lines(
        "local cs = game:GetService('CollectionService') \
         local p = Instance.new('Part', workspace) \
         cs:GetInstanceAddedSignal('x'):Connect(function(i) print('added', i == p) end) \
         cs:AddTag(p, 'x') \
         print(cs:HasTag(p, 'x'), #cs:GetTagged('x'), p:HasTag('x'), cs:GetTags(p)[1]) \
         cs:RemoveTag(p, 'x') \
         print(cs:HasTag(p, 'x'), #cs:GetTagged('x'))",
    );
    assert_eq!(printed, ["added true", "true 1 true x", "false 0"]);
}

#[test]
fn http_service_json_and_guids_but_no_requests() {
    let printed = lines(
        "local http = game:GetService('HttpService') \
         local back = http:JSONDecode(http:JSONEncode({a = {1, 2}, s = 'x', ok = true})) \
         print(back.a[2], back.s, back.ok) \
         print(#http:GenerateGUID(), #http:GenerateGUID(false)) \
         print(http:GenerateGUID(false):match('^%x+%-%x+%-%x+%-%x+%-%x+$') ~= nil) \
         print(pcall(http.GetAsync, http, 'https://example.com'))",
    );
    assert_eq!(printed[..3], ["2 x true", "38 36", "true"]);
    assert!(printed[3].starts_with("false") && printed[3].contains("network"));
}

fn place_with_modules(modules: &[(&str, &str)]) -> WeakDom {
    let mut dom = place();
    let workspace = dom.root_refs()[0];
    for (name, source) in modules {
        let module = dom.new_instance("ModuleScript", name, Some(workspace));
        dom.set_property(module, "Source", Variant::String((*source).to_owned()))
            .unwrap();
    }
    dom
}

#[test]
fn require_runs_a_module_once_per_run() {
    let dom = place_with_modules(&[("M", "print('loading', script.Name) return { n = 1 }")]);
    let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
    let source = "local a = require(workspace.M) print(rawequal(a, require(workspace.M)), a.n)";
    let first = runtime.run(source).unwrap();
    assert_eq!(first.lines(), ["loading M", "true 1"]);
    let second = runtime.run(source).unwrap();
    assert_eq!(
        second.lines(),
        ["loading M", "true 1"],
        "a new run, a fresh cache"
    );
}

#[test]
fn require_reports_cycles_and_invalid_arguments() {
    let dom = place_with_modules(&[
        ("A", "return require(script.Parent.B)"),
        ("B", "return require(script.Parent.A)"),
    ]);
    let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
    let error = runtime.run("require(workspace.A)").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Requested module was required recursively"),
        "{error}"
    );
    let error = runtime.run("require(workspace)").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Attempted to call require with invalid argument(s)."),
        "{error}"
    );
}

#[test]
fn get_service_creates_a_missing_service() {
    let mut runtime = runtime();
    let output = runtime
        .run("local rs = game:GetService('RunService') print(rs.Name, rs == game:GetService('RunService'))")
        .unwrap();
    assert_eq!(output.lines(), ["RunService true"]);
    let dom = runtime.dom();
    assert!(dom
        .root_refs()
        .iter()
        .any(|r| dom.get(*r).unwrap().class() == "RunService"));
    drop(dom);
    let error = runtime.run("game:GetService('NotAService')").unwrap_err();
    assert!(
        error.to_string().contains("is not a valid Service name"),
        "{error}"
    );
}

#[test]
fn loadstring_and_function_environments_work() {
    let printed = lines(
        "loadstring('print(1 + 1)')() \
         loadstring('task.wait(0.01) print(workspace.Name)')() \
         print(type(getfenv), type(setfenv), getfenv(1).workspace == workspace)",
    );
    assert_eq!(printed, ["2", "Workspace", "function function true"]);
}
