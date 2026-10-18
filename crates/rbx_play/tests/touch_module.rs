use mlua::{Function, Lua, Table};

const TOUCH: &str = include_str!("../src/probe/touch.luau");

fn load_module() -> (Lua, Table) {
    let lua = Lua::new();
    let module: Table = lua
        .load(TOUCH)
        .set_name("touch.luau")
        .eval()
        .expect("touch.luau loads");
    (lua, module)
}

fn call<R: mlua::FromLuaMulti>(module: &Table, name: &str, args: impl mlua::IntoLuaMulti) -> R {
    let f: Function = module.get(name).expect("function exists");
    f.call(args).expect("call succeeds")
}

#[test]
fn metrics_double_past_a_500_pixel_short_side() {
    let (_lua, module) = load_module();
    let phone: Table = call(&module, "metrics", (852, 393));
    assert_eq!(phone.get::<f64>("thumbstickSize").unwrap(), 45.0);
    assert_eq!(phone.get::<f64>("maxSpeed").unwrap(), 20.0);
    let tablet: Table = call(&module, "metrics", (1366, 1024));
    assert_eq!(tablet.get::<f64>("thumbstickSize").unwrap(), 90.0);
    assert_eq!(tablet.get::<f64>("deadZone").unwrap(), 4.0);
}

#[test]
fn stick_region_follows_orientation() {
    let (_lua, module) = load_module();
    let (x, y, w, h): (f64, f64, f64, f64) = call(&module, "stickRegion", (852, 393));
    assert_eq!((x, y), (0.0, 131.0));
    assert!((w - 340.8).abs() < 1e-9 && (h - 262.0).abs() < 1e-9);
    let (x, y, w, h): (f64, f64, f64, f64) = call(&module, "stickRegion", (393, 852));
    assert_eq!((x, w), (0.0, 393.0));
    assert!((y - 511.2).abs() < 1e-9 && (h - 340.8).abs() < 1e-9);
}

#[test]
fn jump_button_matches_touch_jump() {
    let (_lua, module) = load_module();
    let small: (f64, f64, f64) = call(&module, "jumpLayout", (852, 393));
    assert_eq!(small, (70.0, -95.0, -90.0));
    let big: (f64, f64, f64) = call(&module, "jumpLayout", (1366, 1024));
    assert_eq!(big, (120.0, -170.0, -210.0));
}

#[test]
fn move_vector_has_a_dead_zone_and_caps_at_full_speed() {
    let (_lua, module) = load_module();
    let m: Table = call(&module, "metrics", (852, 393));
    let (x, z): (f64, f64) = call(&module, "moveVector", (1.0, 1.0, m.clone()));
    assert_eq!((x, z), (0.0, 0.0));
    let (x, z): (f64, f64) = call(&module, "moveVector", (10.0, 0.0, m.clone()));
    assert!((x - 0.5).abs() < 1e-9 && z == 0.0);
    let (x, z): (f64, f64) = call(&module, "moveVector", (0.0, -60.0, m));
    assert!(x == 0.0 && (z + 1.0).abs() < 1e-9);
}

#[test]
fn jump_needs_power_and_the_jumping_state() {
    let (_lua, module) = load_module();
    let yes: bool = call(&module, "canJump", (true, 50.0, 0.0, true));
    let no_power: bool = call(&module, "canJump", (false, 50.0, 0.0, true));
    let disabled: bool = call(&module, "canJump", (true, 50.0, 7.2, false));
    assert!(yes && !no_power && !disabled);
}
