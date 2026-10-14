use mlua::{Function, Lua, Table, Value};

const MARKET_UI: &str = include_str!("../src/probe/market_ui.luau");

const GAME_STUB: &str = "game = { GetService = function(_, n) return {} end }";

fn compiles(name: &str, source: &str) {
    let lua = Lua::new();
    if let Err(err) = lua.load(source).set_name(name).into_function() {
        panic!("{name} does not compile: {err}");
    }
}

fn load_module() -> (Lua, Table) {
    let lua = Lua::new();
    lua.load(GAME_STUB).exec().expect("game stub installs");
    let module: Table = lua
        .load(MARKET_UI)
        .set_name("market_ui.luau")
        .eval()
        .expect("market_ui.luau loads");
    (lua, module)
}

fn bezier_fn(module: &Table, x1: f64, y1: f64, x2: f64, y2: f64) -> Function {
    let bezier: Function = module.get("bezier").expect("bezier exists");
    let easing: Function = bezier
        .call((x1, y1, x2, y2))
        .expect("bezier(x1, y1, x2, y2) returns a function");
    easing
}

fn eval_at(easing: &Function, t: f64) -> f64 {
    let value: f64 = easing.call(t).expect("easing is callable with a number");
    value
}

#[test]
fn market_ui_compiles() {
    compiles("market_ui.luau", MARKET_UI);
}

#[test]
fn module_exports_prompt_bezier_and_is_open() {
    let (_lua, module) = load_module();
    for name in ["prompt", "bezier", "isOpen"] {
        let value: Value = module.get(name).expect("field lookup");
        assert!(
            matches!(value, Value::Function(_)),
            "market_ui.luau must export function {name}"
        );
    }
}

#[test]
fn is_open_is_false_initially() {
    let (_lua, module) = load_module();
    let is_open: Function = module.get("isOpen").expect("isOpen exists");
    let open: bool = is_open.call(()).expect("isOpen() runs");
    assert!(!open, "no dialog is open before any prompt");
}

#[test]
fn bezier_css_ease_hits_endpoints_and_midpoint() {
    let (_lua, module) = load_module();
    let easing = bezier_fn(&module, 0.2, 0.0, 0.0, 1.0);
    assert!((eval_at(&easing, 0.0)).abs() < 1e-9, "f(0) must be 0");
    assert!((eval_at(&easing, 1.0) - 1.0).abs() < 1e-9, "f(1) must be 1");
    let mid = eval_at(&easing, 0.5);
    assert!(
        (mid - 0.8778).abs() < 0.005,
        "cubic-bezier(0.2,0,0,1) at x=0.5 should be ~0.8778, got {mid}"
    );
}

#[test]
fn bezier_css_ease_is_strictly_increasing() {
    let (_lua, module) = load_module();
    let easing = bezier_fn(&module, 0.2, 0.0, 0.0, 1.0);
    let mut previous = eval_at(&easing, 0.0);
    for i in 1..=10 {
        let t = f64::from(i) / 10.0;
        let current = eval_at(&easing, t);
        assert!(
            current > previous,
            "bezier must be strictly increasing: f({t}) = {current} <= {previous}"
        );
        previous = current;
    }
}

#[test]
fn bezier_second_curve_midpoint() {
    let (_lua, module) = load_module();
    let easing = bezier_fn(&module, 0.8, 0.0, 0.4, 1.0);
    let mid = eval_at(&easing, 0.5);
    assert!(
        (mid - 0.2953).abs() < 0.005,
        "cubic-bezier(0.8,0,0.4,1) at x=0.5 should be ~0.2953, got {mid}"
    );
}

#[test]
fn bezier_linear_curve_is_identity() {
    let (_lua, module) = load_module();
    let easing = bezier_fn(&module, 0.0, 0.0, 1.0, 1.0);
    let value = eval_at(&easing, 0.3);
    assert!(
        (value - 0.3).abs() < 1e-3,
        "linear bezier at 0.3 should be 0.3, got {value}"
    );
}

#[test]
fn bezier_clamps_inputs_outside_unit_interval() {
    let (_lua, module) = load_module();
    let easing = bezier_fn(&module, 0.2, 0.0, 0.0, 1.0);
    assert!(
        (eval_at(&easing, -1.0)).abs() < 1e-9,
        "f(-1) must clamp to 0"
    );
    assert!(
        (eval_at(&easing, 2.0) - 1.0).abs() < 1e-9,
        "f(2) must clamp to 1"
    );
}
