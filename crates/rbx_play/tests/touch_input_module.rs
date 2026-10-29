use mlua::{Function, Lua, Table, Value};

const GESTURE: &str = include_str!("../src/probe/touch_gesture.luau");
const PROXY: &str = include_str!("../src/probe/touch_proxy.luau");
const FINGERS: &str = include_str!("../src/probe/touch_fingers.luau");
const TOUCH_INPUT: &str = include_str!("../src/probe/touch_input.luau");

/// The pure half, which the gesture tests below drive.
fn load_module() -> (Lua, Table) {
    let lua = Lua::new();
    let module: Table = lua
        .load(GESTURE)
        .set_name("touch_gesture.luau")
        .eval()
        .expect("touch_gesture.luau loads");
    (lua, module)
}

/// The four modules the way Roblox wires them: each `require`s its siblings
/// by name through `script.Parent:WaitForChild`.
fn load_wired(name: &str) -> (Lua, Value) {
    let lua = Lua::new();
    lua.load("script = { Parent = { WaitForChild = function(_, name) return name end } }")
        .exec()
        .expect("script stub installs");
    let require = lua
        .create_function(|lua, name: String| {
            let source = match name.as_str() {
                "__rbxnative_touch_gesture" => GESTURE,
                "__rbxnative_touch_proxy" => PROXY,
                "__rbxnative_touch_fingers" => FINGERS,
                "__rbxnative_touch_input" => TOUCH_INPUT,
                other => panic!("unexpected require of {other}"),
            };
            lua.load(source).set_name(name).eval::<Value>()
        })
        .expect("require builds");
    lua.globals()
        .set("require", &require)
        .expect("require installs");
    let module = require.call(name).expect("module loads with its siblings");
    (lua, module)
}

#[test]
fn the_split_modules_wire_together() {
    let (_lua, Value::Table(input)) = load_wired("__rbxnative_touch_input") else {
        panic!("touch_input returns its table");
    };
    for name in [
        "start",
        "wrap",
        "event",
        "typeof",
        "setKeyboard",
        "setPointerInside",
    ] {
        input
            .get::<Function>(name)
            .unwrap_or_else(|_| panic!("{name} is exported"));
    }
    let (_lua, Value::Table(proxy)) = load_wired("__rbxnative_touch_proxy") else {
        panic!("touch_proxy returns its table");
    };
    proxy
        .get::<Function>("makeProxy")
        .expect("makeProxy is exported");
    let (_lua, fingers) = load_wired("__rbxnative_touch_fingers");
    assert!(matches!(fingers, Value::Function(_)));
}

fn call<R: mlua::FromLuaMulti>(module: &Table, name: &str, args: impl mlua::IntoLuaMulti) -> R {
    let f: Function = module.get(name).expect("function exists");
    f.call(args).expect("call succeeds")
}

/// `kind` or `kind:state` of each event, in order.
fn kinds(events: &Table) -> Vec<String> {
    events
        .sequence_values::<Table>()
        .map(|e| {
            let e = e.unwrap();
            let kind: String = e.get("kind").unwrap();
            match e.get::<Option<String>>("state").unwrap() {
                Some(state) => format!("{kind}:{state}"),
                None => kind,
            }
        })
        .collect()
}

#[test]
fn quick_still_press_is_a_tap() {
    let (_lua, m) = load_module();
    let f: Table = call(&m, "fingerDown", (100, 100, 0.0));
    let moved: Table = call(&m, "fingerMove", (&f, 104, 103, 0.05));
    assert!(kinds(&moved).is_empty(), "inside the slop");
    let up: Table = call(&m, "fingerUp", (&f, 104, 103, 0.2));
    assert_eq!(kinds(&up), ["Tap"]);
}

#[test]
fn slow_still_press_is_a_long_press_not_a_tap() {
    let (_lua, m) = load_module();
    let f: Table = call(&m, "fingerDown", (100, 100, 0.0));
    let held: Table = call(&m, "fingerHeld", &f);
    assert_eq!(kinds(&held), ["LongPress:Begin"]);
    let moved: Table = call(&m, "fingerMove", (&f, 160, 100, 0.7));
    assert!(kinds(&moved).is_empty(), "a long press does not pan");
    let up: Table = call(&m, "fingerUp", (&f, 160, 100, 0.8));
    assert_eq!(kinds(&up), ["LongPress:End"]);
}

#[test]
fn drag_pans_from_the_press_point() {
    let (_lua, m) = load_module();
    let f: Table = call(&m, "fingerDown", (100, 100, 0.0));
    let first: Table = call(&m, "fingerMove", (&f, 115, 100, 0.1));
    assert_eq!(kinds(&first), ["Pan:Begin"]);
    let pan: Table = first.get(1).unwrap();
    assert_eq!(pan.get::<f64>("tx").unwrap(), 15.0);
    let held: Table = call(&m, "fingerHeld", &f);
    assert!(kinds(&held).is_empty(), "a pan is not a long press");
    let next: Table = call(&m, "fingerMove", (&f, 130, 110, 0.2));
    assert_eq!(kinds(&next), ["Pan:Change"]);
    // Slow at release: no swipe, and no tap after a pan.
    let _: () = call(&m, "settle", (&f, 1.0));
    let up: Table = call(&m, "fingerUp", (&f, 130, 110, 1.0));
    assert_eq!(kinds(&up), ["Pan:End"]);
    let end: Table = up.get(1).unwrap();
    assert_eq!(end.get::<f64>("vx").unwrap(), 0.0);
    assert_eq!(end.get::<f64>("ty").unwrap(), 10.0);
}

#[test]
fn fast_flick_swipes_along_its_main_axis() {
    let (_lua, m) = load_module();
    let f: Table = call(&m, "fingerDown", (300, 300, 0.0));
    let _: Table = call(&m, "fingerMove", (&f, 300, 260, 0.02));
    let _: Table = call(&m, "fingerMove", (&f, 305, 200, 0.04));
    let _: () = call(&m, "settle", (&f, 0.05));
    let up: Table = call(&m, "fingerUp", (&f, 305, 200, 0.05));
    assert_eq!(kinds(&up), ["Pan:End", "Swipe"]);
    let swipe: Table = up.get(2).unwrap();
    assert_eq!(swipe.get::<String>("direction").unwrap(), "Up");
    let dir: String = call(&m, "swipeDirection", (-50, 20));
    assert_eq!(dir, "Left");
    let dir: String = call(&m, "swipeDirection", (5, 60));
    assert_eq!(dir, "Down");
}

#[test]
fn wheel_fingers_spread_and_turn_around_the_cursor() {
    let (_lua, m) = load_module();
    let (x1, y1, x2, y2): (f64, f64, f64, f64) = call(&m, "pinchPositions", (200, 100, 100, 0));
    assert_eq!((x1, y1, x2, y2), (150.0, 100.0, 250.0, 100.0));
    let (x1, y1, x2, y2): (f64, f64, f64, f64) = call(&m, "pinchPositions", (200, 100, 100, 90));
    assert!((x1 - 200.0).abs() < 1e-9 && (x2 - 200.0).abs() < 1e-9);
    assert!((y1 - 50.0).abs() < 1e-9 && (y2 - 150.0).abs() < 1e-9);

    let (span, angle): (f64, f64) = call(&m, "wheelStep", (100, 0, true, false));
    assert!((span - 110.0).abs() < 1e-9 && angle == 0.0);
    let (span, _): (f64, f64) = call(&m, "wheelStep", (110, 0, false, false));
    assert!((span - 100.0).abs() < 1e-9);
    let (span, angle): (f64, f64) = call(&m, "wheelStep", (100, 0, false, true));
    assert_eq!((span, angle), (100.0, -15.0));
}

#[test]
fn signal_connects_disconnects_and_fires_once() {
    let (lua, m) = load_module();
    let (signal, fire): (Table, Function) = call(&m, "signal", ());
    lua.globals().set("signal", signal).unwrap();
    lua.globals().set("fire", fire).unwrap();
    let seen: Value = lua
        .load(
            r#"
            local seen = {}
            local c = signal:Connect(function(v) table.insert(seen, "a" .. v) end)
            signal:Once(function(v) table.insert(seen, "o" .. v) end)
            fire(1)
            c:Disconnect()
            fire(2)
            return table.concat(seen, ",") .. "|" .. tostring(c.Connected)
            "#,
        )
        .eval()
        .unwrap();
    assert_eq!(seen.as_string().unwrap().to_str().unwrap(), "a1,o1|false");
}

#[test]
fn stand_in_touch_reports_changes_like_an_input_object() {
    let (lua, m) = load_module();
    lua.globals().set("M", m).unwrap();
    // Roblox's typeof, for the fallback.
    lua.load("typeof = function(v) return type(v) end")
        .exec()
        .unwrap();
    let seen: String = lua
        .load(
            r#"
            local touch = M.newTouch({ UserInputState = "Begin", Position = 1 })
            local seen = {}
            touch.Changed:Connect(function(p) table.insert(seen, p) end)
            touch:GetPropertyChangedSignal("UserInputState"):Connect(function()
                table.insert(seen, "state=" .. touch.UserInputState)
            end)
            M.setTouch(touch, "Position", 2)
            M.setTouch(touch, "Position", 2) -- no change, no signal
            M.setTouch(touch, "UserInputState", "End")
            table.insert(seen, touch.ClassName)
            table.insert(seen, tostring(touch:IsA("InputObject")) .. "/" .. tostring(touch:IsA("Part")))
            table.insert(seen, M.typeof(touch) .. "/" .. M.typeof(1))
            table.insert(seen, tostring(touch))
            return table.concat(seen, ",")
            "#,
        )
        .eval()
        .unwrap();
    assert_eq!(
        seen,
        "Position,UserInputState,state=End,InputObject,true/false,Instance/number,InputObject"
    );
}

#[test]
fn touch_rotation_matches_camera_input() {
    let (_lua, m) = load_module();
    let one = 1f64.to_radians();
    // Level camera: a degree per pixel across, 0.66 down.
    let (yaw, pitch): (f64, f64) = call(&m, "touchRotation", (10, 10, 0.0));
    assert!((yaw - 10.0 * one).abs() < 1e-12 && (pitch - 6.6 * one).abs() < 1e-12);
    // Looking up and dragging up (away from the horizon): slowed.
    let (_, slowed): (f64, f64) = call(&m, "touchRotation", (0, -10, 0.5));
    assert!(slowed.abs() < 6.6 * one && slowed < 0.0);
    // Dragging back towards the horizon: full speed.
    let (_, full): (f64, f64) = call(&m, "touchRotation", (0, 10, 0.5));
    assert!((full - 6.6 * one).abs() < 1e-12);
    // BaseCamera's ±80° limit.
    let clamped: f64 = call(&m, "clampPitch", (2.0, 0.0));
    assert!((clamped - 80f64.to_radians()).abs() < 1e-12);
}

fn keyboard(m: &Table, rect: (i32, i32, i32, i32)) -> Table {
    call(m, "keyboardProps", rect)
}

fn keyboard_changes(m: &Table, old: &Table, new: &Table) -> Vec<String> {
    call::<Table>(m, "keyboardChanges", (old, new))
        .sequence_values::<String>()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn keyboard_cover_reports_visible_position_and_size() {
    let (_lua, m) = load_module();
    let up = keyboard(&m, (0, 552, 393, 300));
    assert!(up.get::<bool>("OnScreenKeyboardVisible").unwrap());
    let position: Vec<i32> = up.get("OnScreenKeyboardPosition").unwrap();
    let size: Vec<i32> = up.get("OnScreenKeyboardSize").unwrap();
    assert_eq!((position, size), (vec![0, 552], vec![393, 300]));

    // A cover with no height is a keyboard that is down, all zero.
    let down = keyboard(&m, (0, 852, 393, 0));
    assert!(!down.get::<bool>("OnScreenKeyboardVisible").unwrap());
    let position: Vec<i32> = down.get("OnScreenKeyboardPosition").unwrap();
    assert_eq!(position, [0, 0]);
}

#[test]
fn keyboard_changes_name_only_what_moved() {
    let (_lua, m) = load_module();
    let down = keyboard(&m, (0, 0, 0, 0));
    let up = keyboard(&m, (0, 552, 393, 300));
    assert_eq!(
        keyboard_changes(&m, &down, &up),
        [
            "OnScreenKeyboardVisible",
            "OnScreenKeyboardPosition",
            "OnScreenKeyboardSize"
        ]
    );
    // Rotated: a new rect, still up.
    let turned = keyboard(&m, (0, 193, 852, 200));
    assert_eq!(
        keyboard_changes(&m, &up, &turned),
        ["OnScreenKeyboardPosition", "OnScreenKeyboardSize"]
    );
    assert!(keyboard_changes(&m, &up, &keyboard(&m, (0, 552, 393, 300))).is_empty());
}
