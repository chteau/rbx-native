use rbx_dom::WeakDom;

use super::*;
use crate::plugin::api::OutputKind;
use crate::plugin::testing::test_vm;

fn run(lua: &Lua, src: &str) {
    spawn(
        lua,
        lua.load(src).into_function().unwrap(),
        MultiValue::new(),
    )
    .unwrap();
}

#[test]
fn wait_resolves_only_after_time_passes() {
    let (lua, _env, _) = test_vm(WeakDom::new(), false);
    run(
        &lua,
        "done = false local e = task.wait(1) done = true elapsed = e",
    );
    assert!(!lua.globals().get::<bool>("done").unwrap());
    tick(&lua, 0.5).unwrap();
    assert!(!lua.globals().get::<bool>("done").unwrap());
    tick(&lua, 1.25).unwrap();
    assert!(lua.globals().get::<bool>("done").unwrap());
    assert_eq!(lua.globals().get::<f64>("elapsed").unwrap(), 1.25);
}

#[test]
fn delay_runs_function_with_args() {
    let (lua, _env, _) = test_vm(WeakDom::new(), false);
    run(&lua, "task.delay(1, function(x) got = x end, 5)");
    tick(&lua, 0.5).unwrap();
    assert!(lua.globals().get::<Option<f64>>("got").unwrap().is_none());
    tick(&lua, 2.0).unwrap();
    assert_eq!(lua.globals().get::<f64>("got").unwrap(), 5.0);
}

#[test]
fn cancel_stops_delay() {
    let (lua, _env, _) = test_vm(WeakDom::new(), false);
    run(
        &lua,
        "task.cancel(task.delay(1, function() got = true end))",
    );
    tick(&lua, 2.0).unwrap();
    assert!(lua.globals().get::<Option<bool>>("got").unwrap().is_none());
}

#[test]
fn defer_runs_on_next_tick() {
    let (lua, env, _) = test_vm(WeakDom::new(), false);
    run(&lua, "task.defer(function() d = true end)");
    assert!(lua.globals().get::<Option<bool>>("d").unwrap().is_none());
    assert!(wants_tick(&env.engine.scheduler.borrow(), 0.0));
    tick(&lua, 0.0).unwrap();
    assert!(lua.globals().get::<bool>("d").unwrap());
}

#[test]
fn errors_become_output() {
    let (lua, env, _) = test_vm(WeakDom::new(), false);
    run(&lua, "error('boom')");
    let shared = env.shared.borrow();
    assert_eq!(shared.output.len(), 1);
    assert_eq!(shared.output[0].kind, OutputKind::Error);
    assert_eq!(shared.output[0].source, "Test");
    assert!(shared.output[0].text.contains("boom"));
}

#[test]
fn resolve_error_raises_in_thread() {
    let (lua, env, _) = test_vm(WeakDom::new(), false);
    let f = yielding(
        &lua,
        lua.create_function(|lua, ()| Ok(new_await(lua))).unwrap(),
    )
    .unwrap();
    lua.globals().set("op", f).unwrap();
    run(&lua, "local ok, e = pcall(op) result = e");
    let id = *env.engine.scheduler.borrow().waiting.keys().next().unwrap();
    resolve(&lua, id, Err("nope".into())).unwrap();
    assert!(lua
        .globals()
        .get::<String>("result")
        .unwrap()
        .contains("nope"));
}
