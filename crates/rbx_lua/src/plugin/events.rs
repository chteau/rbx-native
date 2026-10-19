//! Instance events and the plugin-only members of instances, through
//! `crate::hooks`.

use std::collections::HashMap;

use mlua::{Function, Lua, MultiValue, Result, Value};
use rbx_dom::{Ref, Vector2Data};
use rbx_reflection::ReflectionDatabase;

use super::api::{Arg, GuiEvent, GuiEventKind, HandleEvent, HandleEventKind, OutputKind};
use super::arg;
use super::env::env;
use crate::ctx::Ctx;
use crate::datatypes::misc::LuaVector2;
use crate::engine::changes;
use crate::engine::events::{self, instance_key, ref_key, registry_table};
use crate::engine::signal::{self, LuaSignal, Owner, SignalKey};
use crate::hooks::Hooks;

const BIND_TO_CLOSE_KEY: &str = "__plugin_bind_to_close";

const GUI_OBJECT_SIGNALS: &[&str] = &[
    "MouseEnter",
    "MouseLeave",
    "MouseMoved",
    "MouseButton1Down",
    "MouseButton1Up",
    "MouseButton2Down",
    "MouseButton2Up",
    "MouseWheelForward",
    "MouseWheelBackward",
    "InputBegan",
    "InputEnded",
    "InputChanged",
    "DragBegin",
    "DragStopped",
    "SelectionGained",
    "SelectionLost",
    "TouchTap",
];
const GUI_BUTTON_SIGNALS: &[&str] = &["MouseButton1Click", "MouseButton2Click", "Activated"];
const TEXT_BOX_SIGNALS: &[&str] = &["Focused", "FocusLost"];
const HANDLES_SIGNALS: &[&str] = &[
    "MouseButton1Down",
    "MouseButton1Up",
    "MouseDrag",
    "MouseEnter",
    "MouseLeave",
];
const HANDLE_ADORNMENT_SIGNALS: &[&str] = &[
    "MouseButton1Down",
    "MouseButton1Up",
    "MouseEnter",
    "MouseLeave",
];
const PLUGIN_GUI_SIGNALS: &[&str] = &["WindowFocused", "WindowFocusReleased"];
const NORMAL_IDS: [&str; 6] = ["Right", "Top", "Back", "Left", "Bottom", "Front"];
const AXES: [&str; 3] = ["X", "Y", "Z"];

/// Per-VM GUI state the DOM has no property for.
#[derive(Default)]
struct EventState {
    focused: Option<Ref>,
    /// Last pointer position, by element and by the widget root above it.
    pointer: HashMap<Ref, (f32, f32)>,
    /// Last pointer position seen on any element, and whether mouse
    /// buttons 1 and 2 are held.
    mouse: (f32, f32),
    buttons: [bool; 2],
}

/// The last pointer position and the held state of mouse buttons 1 and 2.
pub(crate) fn pointer_state(lua: &Lua) -> ((f32, f32), [bool; 2]) {
    with_state(lua, |s| (s.mouse, s.buttons))
}

/// The `TextBox` that currently has focus.
pub(crate) fn focused_text_box(lua: &Lua) -> Option<Ref> {
    with_state(lua, |s| s.focused)
}

/// Fires the `Input*` signals on the element and on `UserInputService`.
/// `UserInputService` events for the 3D view's mouse (what Roblox reports
/// for input over the viewport), at `at` in the view's pixels.
pub(crate) fn fire_viewport_input(
    lua: &Lua,
    input: crate::plugin::api::MouseInput,
    at: [f32; 2],
) -> Result<()> {
    use crate::plugin::api::MouseInput;
    let [x, y] = at;
    let button = |b: u8| {
        if b == 2 {
            "MouseButton2"
        } else {
            "MouseButton1"
        }
    };
    let (signal_name, kind, state, z) = match input {
        MouseInput::Down(b) => {
            with_state(lua, |s| s.buttons[usize::from(b == 2)] = true);
            ("InputBegan", button(b), "Begin", 0.0)
        }
        MouseInput::Up(b) => {
            with_state(lua, |s| s.buttons[usize::from(b == 2)] = false);
            ("InputEnded", button(b), "End", 0.0)
        }
        MouseInput::Move => ("InputChanged", "MouseMovement", "Change", 0.0),
        MouseInput::Wheel(dir) => ("InputChanged", "MouseWheel", "Change", f32::from(dir)),
    };
    with_state(lua, |s| s.mouse = (x, y));
    let object = super::input::input_object_at(lua, kind, state, [x, y, z])?;
    signal::fire_values(
        lua,
        &SignalKey::new(Owner::Service("UserInputService"), signal_name),
        MultiValue::from_iter([object, Value::Boolean(false)]),
    )
}

fn fire_input(
    lua: &Lua,
    r: Ref,
    signal_name: &str,
    kind: &str,
    state: &str,
    position: [f32; 3],
) -> Result<()> {
    let [x, y, z] = position;
    let input = if z == 0.0 {
        super::input::input_object(lua, kind, state, x, y)?
    } else {
        super::input::input_object_at(lua, kind, state, position)?
    };
    signal::fire_values(
        lua,
        &instance_key(r, signal_name),
        MultiValue::from_iter([input.clone()]),
    )?;
    signal::fire_values(
        lua,
        &SignalKey::new(Owner::Service("UserInputService"), signal_name),
        MultiValue::from_iter([input, Value::Boolean(false)]),
    )
}

fn fire_gui_input(lua: &Lua, r: Ref, kind: &GuiEventKind) -> Result<()> {
    let button = |b: u8| {
        if b == 2 {
            "MouseButton2"
        } else {
            "MouseButton1"
        }
    };
    match kind {
        GuiEventKind::Enter { x, y } => {
            with_state(lua, |s| s.mouse = (*x, *y));
            fire_input(
                lua,
                r,
                "InputBegan",
                "MouseMovement",
                "Begin",
                [*x, *y, 0.0],
            )
        }
        GuiEventKind::Leave { x, y } => {
            with_state(lua, |s| s.mouse = (*x, *y));
            fire_input(lua, r, "InputEnded", "MouseMovement", "End", [*x, *y, 0.0])
        }
        GuiEventKind::Move { x, y } => {
            with_state(lua, |s| s.mouse = (*x, *y));
            fire_input(
                lua,
                r,
                "InputChanged",
                "MouseMovement",
                "Change",
                [*x, *y, 0.0],
            )
        }
        GuiEventKind::Down { button: b, x, y } => {
            with_state(lua, |s| {
                s.mouse = (*x, *y);
                s.buttons[usize::from(*b == 2)] = true;
            });
            fire_input(lua, r, "InputBegan", button(*b), "Begin", [*x, *y, 0.0])
        }
        GuiEventKind::Up { button: b, x, y } => {
            with_state(lua, |s| {
                s.mouse = (*x, *y);
                s.buttons[usize::from(*b == 2)] = false;
            });
            fire_input(lua, r, "InputEnded", button(*b), "End", [*x, *y, 0.0])
        }
        GuiEventKind::Scroll { dy } => {
            let (x, y) = with_state(lua, |s| s.pointer.get(&r).copied().unwrap_or(s.mouse));
            let z = if *dy > 0.0 {
                -1.0
            } else if *dy < 0.0 {
                1.0
            } else {
                0.0
            };
            fire_input(lua, r, "InputChanged", "MouseWheel", "Change", [x, y, z])
        }
        _ => Ok(()),
    }
}

fn with_state<R>(lua: &Lua, f: impl FnOnce(&mut EventState) -> R) -> R {
    if lua.app_data_ref::<EventState>().is_none() {
        lua.set_app_data(EventState::default());
    }
    let mut state = lua
        .app_data_mut::<EventState>()
        .expect("event state was just installed");
    f(&mut state)
}

/// The hooks every plugin VM installs.
///
/// `index` answers what `engine::events::index` does (instance signals,
/// attributes, bindables, remotes), then, by class
/// (`Ctx::database().is_subclass_of`):
/// - every instance: `WaitForChild(name, timeout?)` (the child, or nil
///   after a warning — nothing loads later in the editor);
/// - `GuiObject`: `MouseEnter`, `MouseLeave`, `MouseMoved`,
///   `MouseButton1Down`, `MouseButton1Up`, `MouseButton2Down`,
///   `MouseButton2Up`, `MouseWheelForward`, `MouseWheelBackward`,
///   `InputBegan`, `InputEnded`, `InputChanged`; `GuiButton`:
///   `MouseButton1Click`, `MouseButton2Click`, `Activated`; `TextBox`:
///   `Focused`, `FocusLost`, `CaptureFocus()`, `ReleaseFocus()`,
///   `IsFocused()`;
/// - `HandlesBase` (`Handles`, `ArcHandles`): `MouseButton1Down`,
///   `MouseButton1Up`, `MouseDrag`, `MouseEnter`, `MouseLeave`;
///   `HandleAdornment`: `MouseButton1Down`, `MouseButton1Up`,
///   `MouseEnter`, `MouseLeave`;
/// - `PluginGui` / `DockWidgetPluginGui` roots: `BindToClose(fn?)`,
///   `WindowFocused`, `WindowFocusReleased`, `GetRelativeMousePosition()`,
///   `HostWidgetWasRestored` (false).
///
/// `new_index` handles the invoke callbacks (`engine::events::new_index`),
/// and for a widget root keeps `Ui` in step with `Title`
/// and `Enabled` (returning false so the property is written too).
/// `service` delegates to `services::service`.
pub(crate) struct PluginHooks;

fn is_handles(db: &ReflectionDatabase, class: &str) -> bool {
    matches!(class, "Handles" | "ArcHandles") || db.is_subclass_of(class, "HandlesBase")
}

/// The signals only plugin VMs give instances: GUI input, focus and
/// handles (the shared ones are `engine::events::is_signal`).
fn is_plugin_signal(db: &ReflectionDatabase, class: &str, key: &str) -> bool {
    let has = |list: &[&str], base: &str| list.contains(&key) && db.is_subclass_of(class, base);
    has(GUI_OBJECT_SIGNALS, "GuiObject")
        || has(GUI_BUTTON_SIGNALS, "GuiButton")
        || has(TEXT_BOX_SIGNALS, "TextBox")
        || (HANDLES_SIGNALS.contains(&key) && is_handles(db, class))
        || has(HANDLE_ADORNMENT_SIGNALS, "HandleAdornment")
        || has(PLUGIN_GUI_SIGNALS, "PluginGui")
}

/// `WaitForChild(name, timeout?)`: the child, or nil after a warning —
/// nothing loads later in the editor.
fn instance_function(lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match key {
        "WaitForChild" => lua.create_function(
            move |lua, (_this, name, timeout): (Value, String, Option<f64>)| {
                let (found, parent_name) = {
                    let dom = ctx.dom();
                    let instance = dom.get(referent).ok_or_else(crate::property::missing_instance)?;
                    let found = instance
                        .children()
                        .iter()
                        .copied()
                        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name));
                    (found, instance.name().to_string())
                };
                match found {
                    Some(child) => arg::to_lua(lua, &ctx, &Arg::Instance(child)),
                    None => {
                        if timeout.is_none() {
                            let env = env(lua);
                            env.shared.borrow_mut().push_output(
                                &env.name,
                                OutputKind::Warn,
                                format!("Infinite yield possible on '{parent_name}:WaitForChild(\"{name}\")'"),
                            );
                        }
                        Ok(Value::Nil)
                    }
                }
            },
        )?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

fn class_function(
    lua: &Lua,
    db: &ReflectionDatabase,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let is = |base: &str| db.is_subclass_of(class, base);
    let f = match key {
        "CaptureFocus" if is("TextBox") => lua.create_function(move |lua, _this: Value| {
            with_state(lua, |s| s.focused = Some(referent));
            signal::fire(lua, &instance_key(referent, "Focused"), &[])
        })?,
        "ReleaseFocus" if is("TextBox") => lua.create_function(move |lua, _this: Value| {
            let was = with_state(lua, |s| {
                let was = s.focused == Some(referent);
                if was {
                    s.focused = None;
                }
                was
            });
            if was {
                signal::fire(
                    lua,
                    &instance_key(referent, "FocusLost"),
                    &[Arg::Bool(false), Arg::Nil],
                )?;
            }
            Ok(())
        })?,
        "IsFocused" if is("TextBox") => lua.create_function(move |lua, _this: Value| {
            Ok(with_state(lua, |s| s.focused == Some(referent)))
        })?,
        "BindToClose" if is("PluginGui") => {
            lua.create_function(move |lua, (_this, callback): (Value, Option<Function>)| {
                registry_table(lua, BIND_TO_CLOSE_KEY)?.set(ref_key(referent), callback)
            })?
        }
        "GetRelativeMousePosition" if is("PluginGui") => {
            lua.create_function(move |lua, _this: Value| {
                let (x, y) = with_state(lua, |s| {
                    s.pointer.get(&referent).copied().unwrap_or((0.0, 0.0))
                });
                lua.create_userdata(LuaVector2(Vector2Data { x, y }))
            })?
        }
        "HostWidgetWasRestored" if is("PluginGui") => return Ok(Some(Value::Boolean(false))),
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// `plugin[key]`, with methods rebound so `instance:Method(...)` calls them
/// on the `plugin` global instead of the instance.
fn plugin_member(lua: &Lua, key: &str) -> Result<Value> {
    let plugin: Value = lua.globals().get("plugin")?;
    let Value::UserData(object) = &plugin else {
        return Ok(Value::Nil);
    };
    match mlua::ObjectLike::get::<Value>(object, key)? {
        Value::Function(method) => lua
            .create_function(move |_, (_this, rest): (Value, mlua::MultiValue)| {
                let mut args = rest;
                args.push_front(plugin.clone());
                method.call::<mlua::MultiValue>(args)
            })
            .map(Value::Function),
        other => Ok(other),
    }
}

impl Hooks for PluginHooks {
    fn game_member(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        events::game_member(lua, key)
    }

    fn index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>> {
        // The `Plugin` instance a model plugin's scripts sit under (see
        // `dispatch::start`) is the `plugin` global seen from the tree, so
        // `script:FindFirstAncestorWhichIsA("Plugin")` reaches the same API.
        if class == "Plugin" && env(lua).roots.borrow().first() == Some(&referent) {
            return plugin_member(lua, key).map(Some);
        }
        if key == "ViewportSize" && class == "Camera" {
            let size = env(lua).shared.borrow().context.viewport_size;
            if let Some([x, y]) = size {
                return lua
                    .create_userdata(LuaVector2(Vector2Data { x, y }))
                    .map(|u| Some(Value::UserData(u)));
            }
        }
        if let Some(value) = events::index(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        let db = ctx.database();
        if is_plugin_signal(db, class, key) {
            return lua
                .create_userdata(LuaSignal(instance_key(referent, key)))
                .map(|u| Some(Value::UserData(u)));
        }
        if let Some(value) = instance_function(lua, ctx, referent, key)? {
            return Ok(Some(value));
        }
        if let Some(value) = class_function(lua, db, referent, class, key)? {
            return Ok(Some(value));
        }
        super::world::class_member(lua, ctx, referent, class, key)
    }

    fn new_index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
        value: &Value,
    ) -> Result<bool> {
        if events::new_index(lua, ctx, referent, class, key, value)? {
            return Ok(true);
        }
        let db = ctx.database();
        if matches!(key, "Title" | "Enabled") && db.is_subclass_of(class, "PluginGui") {
            let env = env(lua);
            let mut shared = env.shared.borrow_mut();
            let mut changed = false;
            if let Some(widget) = shared.ui.widgets.iter_mut().find(|w| w.root == referent) {
                match (key, value) {
                    ("Title", Value::String(s)) => {
                        widget.title = s.to_string_lossy().to_string();
                        changed = true;
                    }
                    ("Enabled", Value::Boolean(b)) => {
                        widget.enabled = *b;
                        changed = true;
                    }
                    _ => {}
                }
            }
            if changed {
                shared.ui_changed = true;
            }
        }
        Ok(false)
    }

    fn service(&self, lua: &Lua, _ctx: &Ctx, name: &str) -> Result<Option<Value>> {
        super::services::service(lua, name)
    }
}

fn emit(lua: &Lua, referent: Ref, event: &str, args: &[Arg]) -> Result<()> {
    signal::fire(lua, &instance_key(referent, event), args)
}

/// Remembers the pointer for `referent` and for the widget root above it.
fn remember_pointer(lua: &Lua, referent: Ref, x: f32, y: f32) {
    let env = env(lua);
    let ancestors = changes::chain(&env.ctx.dom(), Some(referent));
    let root = {
        let shared = env.shared.borrow();
        ancestors
            .into_iter()
            .find(|a| shared.ui.widgets.iter().any(|w| w.root == *a))
    };
    with_state(lua, |s| {
        s.pointer.insert(referent, (x, y));
        if let Some(root) = root {
            s.pointer.insert(root, (x, y));
        }
    });
}

/// Fires the GUI signals for one widget input. `Click` → `MouseButton1Click`
/// (`button` 1) or `MouseButton2Click`, then `Activated` on a `GuiButton`;
/// `Down`/`Up` → `MouseButton{n}Down/Up(x, y)`; `Enter`/`Leave`/`Move` →
/// `MouseEnter`/`MouseLeave`/`MouseMoved(x, y)`; `TextChanged(text)` →
/// writes `Text` (the change log then fires `Changed`); `Focused` /
/// `FocusLost { enter }` → `Focused()` / `FocusLost(enter, nil)`;
/// `Scroll` → `MouseWheelForward`/`Backward`. Also remembers the last
/// pointer position per widget for `GetRelativeMousePosition`.
pub(crate) fn fire_gui(lua: &Lua, event: &GuiEvent) -> Result<()> {
    let r = event.referent;
    let ctx = env(lua).ctx.clone();
    let class = match ctx.dom().get(r) {
        Some(instance) => instance.class().to_string(),
        None => return Ok(()),
    };
    fire_gui_input(lua, r, &event.kind)?;
    let at = |x: f32, y: f32| [Arg::Number(f64::from(x)), Arg::Number(f64::from(y))];
    let button_name = |b: u8| if b == 2 { 2 } else { 1 };
    match &event.kind {
        GuiEventKind::Enter { x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(lua, r, "MouseEnter", &at(*x, *y))
        }
        GuiEventKind::Leave { x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(lua, r, "MouseLeave", &at(*x, *y))
        }
        GuiEventKind::Move { x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(lua, r, "MouseMoved", &at(*x, *y))
        }
        GuiEventKind::Down { button, x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(
                lua,
                r,
                &format!("MouseButton{}Down", button_name(*button)),
                &at(*x, *y),
            )
        }
        GuiEventKind::Up { button, x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(
                lua,
                r,
                &format!("MouseButton{}Up", button_name(*button)),
                &at(*x, *y),
            )
        }
        GuiEventKind::Click { button, x, y } => {
            remember_pointer(lua, r, *x, *y);
            emit(
                lua,
                r,
                &format!("MouseButton{}Click", button_name(*button)),
                &[],
            )?;
            if *button != 2 && ctx.database().is_subclass_of(&class, "GuiButton") {
                emit(lua, r, "Activated", &[])?;
            }
            Ok(())
        }
        GuiEventKind::Focused => {
            with_state(lua, |s| s.focused = Some(r));
            emit(lua, r, "Focused", &[])
        }
        GuiEventKind::TextChanged(text) => {
            let value = Value::String(lua.create_string(text)?);
            crate::property::set(&ctx, r, "Text", &value).map(|_| ())
        }
        GuiEventKind::FocusLost { enter } => {
            with_state(lua, |s| {
                if s.focused == Some(r) {
                    s.focused = None;
                }
            });
            emit(lua, r, "FocusLost", &[Arg::Bool(*enter), Arg::Nil])
        }
        GuiEventKind::Scroll { dy } => {
            let event = if *dy > 0.0 {
                "MouseWheelBackward"
            } else {
                "MouseWheelForward"
            };
            let (x, y) = with_state(lua, |s| s.pointer.get(&r).copied().unwrap_or((0.0, 0.0)));
            emit(lua, r, event, &at(x, y))
        }
    }
}

fn enum_arg(enum_name: &str, names: &[&str], value: u8) -> Arg {
    Arg::Enum {
        enum_name: enum_name.to_string(),
        item: names
            .get(usize::from(value))
            .copied()
            .unwrap_or(names[0])
            .to_string(),
    }
}

/// Fires `Handles`/`ArcHandles`/`HandleAdornment` signals: `Down{face}` →
/// `MouseButton1Down(Enum.NormalId)`, `Drag{face, distance}` →
/// `MouseDrag(Enum.NormalId, distance)`, `ArcDrag{axis, angle,
/// delta_radius}` → `MouseDrag(Enum.Axis, angle, deltaRadius)`, and so on.
pub(crate) fn fire_handle(lua: &Lua, event: &HandleEvent) -> Result<()> {
    let r = event.referent;
    let face = |f: u8| enum_arg("NormalId", &NORMAL_IDS, f);
    let axis = |a: u8| enum_arg("Axis", &AXES, a);
    match &event.kind {
        HandleEventKind::Down { face: f } => emit(lua, r, "MouseButton1Down", &[face(*f)]),
        HandleEventKind::Drag { face: f, distance } => emit(
            lua,
            r,
            "MouseDrag",
            &[face(*f), Arg::Number(f64::from(*distance))],
        ),
        HandleEventKind::Up { face: f } => emit(lua, r, "MouseButton1Up", &[face(*f)]),
        HandleEventKind::Enter { face: f } => emit(lua, r, "MouseEnter", &[face(*f)]),
        HandleEventKind::Leave { face: f } => emit(lua, r, "MouseLeave", &[face(*f)]),
        HandleEventKind::ArcDown { axis: a } => emit(lua, r, "MouseButton1Down", &[axis(*a)]),
        HandleEventKind::ArcDrag {
            axis: a,
            angle,
            delta_radius,
        } => emit(
            lua,
            r,
            "MouseDrag",
            &[
                axis(*a),
                Arg::Number(f64::from(*angle)),
                Arg::Number(f64::from(*delta_radius)),
            ],
        ),
        HandleEventKind::ArcUp { axis: a } => emit(lua, r, "MouseButton1Up", &[axis(*a)]),
        HandleEventKind::ArcEnter { axis: a } => emit(lua, r, "MouseEnter", &[axis(*a)]),
        HandleEventKind::ArcLeave { axis: a } => emit(lua, r, "MouseLeave", &[axis(*a)]),
        HandleEventKind::AdornmentDown => emit(lua, r, "MouseButton1Down", &[]),
        HandleEventKind::AdornmentUp => emit(lua, r, "MouseButton1Up", &[]),
        HandleEventKind::AdornmentEnter => emit(lua, r, "MouseEnter", &[]),
        HandleEventKind::AdornmentLeave => emit(lua, r, "MouseLeave", &[]),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use rbx_dom::{Variant, WeakDom};

    use super::*;
    use crate::engine::scheduler;
    use crate::instance::LuaInstance;
    use crate::plugin::testing::test_vm;

    struct Vm {
        lua: Lua,
        slot: Rc<RefCell<WeakDom>>,
    }

    fn setup(build: impl FnOnce(&mut WeakDom) -> Vec<(&'static str, Ref)>) -> Vm {
        let mut dom = WeakDom::new();
        let globals = build(&mut dom);
        let (lua, env, slot) = test_vm(dom, true);
        slot.borrow_mut().take_changes();
        for (name, r) in globals {
            lua.globals()
                .set(name, LuaInstance::new(r, env.ctx.clone()))
                .unwrap();
        }
        Vm { lua, slot }
    }

    impl Vm {
        fn run(&self, src: &str) {
            let f = self.lua.load(src).into_function().unwrap();
            scheduler::spawn(&self.lua, f, MultiValue::new()).unwrap();
        }

        fn flush(&self) {
            let (changes, first) = changes::take_changes(&mut self.slot.borrow_mut());
            changes::fire_changes(&self.lua, &changes, first).unwrap();
            env(&self.lua).engine.signals.borrow_mut().settle();
        }

        fn get<T: mlua::FromLua>(&self, name: &str) -> T {
            self.lua.globals().get::<T>(name).unwrap()
        }
    }

    #[test]
    fn changed_and_property_signal_fire() {
        let vm = setup(|dom| vec![("f", dom.new_instance("Folder", "F", None))]);
        vm.run(
            "f.Changed:Connect(function(p) changed = p end) \
             f:GetPropertyChangedSignal('Name'):Connect(function() named = true end) \
             f.Name = 'G'",
        );
        vm.flush();
        assert_eq!(vm.get::<String>("changed"), "Name");
        assert!(vm.get::<bool>("named"));
    }

    #[test]
    fn changed_names_the_property_not_its_stored_spelling() {
        let vm = setup(|dom| vec![("p", dom.new_instance("Part", "P", None))]);
        vm.run(
            "seen = {} \
             p.Changed:Connect(function(k) table.insert(seen, k) end) \
             p:GetPropertyChangedSignal('Size'):Connect(function() sized = true end) \
             p.Size = Vector3.new(2, 2, 2) \
             p.Color = Color3.new(1, 0, 0)",
        );
        vm.flush();
        assert!(vm.get::<bool>("sized"));
        let seen: Vec<String> = vm.get("seen");
        assert!(seen.contains(&"Size".to_owned()), "{seen:?}");
        assert!(seen.contains(&"Color".to_owned()), "{seen:?}");
    }

    #[test]
    fn child_added_and_ancestry_changed() {
        let vm = setup(|dom| {
            let ws = dom.new_instance("Workspace", "Workspace", None);
            let f = dom.new_instance("Folder", "F", None);
            vec![("ws", ws), ("f", f)]
        });
        vm.run(
            "ws.ChildAdded:Connect(function(c) added = c.Name end) \
             f.AncestryChanged:Connect(function(c, p) anc = p.Name end) \
             f.Parent = ws",
        );
        vm.flush();
        assert_eq!(vm.get::<String>("added"), "F");
        assert_eq!(vm.get::<String>("anc"), "Workspace");
    }

    #[test]
    fn attribute_round_trip() {
        let vm = setup(|dom| vec![("f", dom.new_instance("Folder", "F", None))]);
        vm.run(
            "f:GetAttributeChangedSignal('Speed'):Connect(function() fired = true end) \
             f:SetAttribute('Speed', 12) f:SetAttribute('Label', 'hi') \
             speed = f:GetAttribute('Speed') label = f:GetAttributes().Label \
             missing = f:GetAttribute('Nope') == nil",
        );
        vm.flush();
        assert_eq!(vm.get::<f64>("speed"), 12.0);
        assert_eq!(vm.get::<String>("label"), "hi");
        assert!(vm.get::<bool>("missing"));
        assert!(vm.get::<bool>("fired"));
    }

    #[test]
    fn bindable_event_fire() {
        let vm = setup(|dom| vec![("b", dom.new_instance("BindableEvent", "B", None))]);
        vm.run("b.Event:Connect(function(a, c) got = a + c end) b:Fire(2, 5)");
        assert_eq!(vm.get::<f64>("got"), 7.0);
    }

    #[test]
    fn a_listener_connected_after_a_change_does_not_hear_it() {
        let vm = setup(|dom| {
            let ws = dom.new_instance("Workspace", "Workspace", None);
            vec![("ws", ws)]
        });
        vm.run(
            "n, added = 0, 0 \
             p = Instance.new('Part') \
             p.Changed:Connect(function() n += 1 end) \
             p.Name = 'A' \
             p.Parent = ws \
             ws.DescendantAdded:Connect(function() added += 1 end)",
        );
        vm.flush();
        // Of the part's own changes, only the rename came after the
        // connection; the parenting came before `DescendantAdded`'s.
        assert_eq!(vm.get::<f64>("n"), 1.0);
        assert_eq!(vm.get::<f64>("added"), 0.0);

        vm.run("p.Name = 'Later' Instance.new('Folder', ws)");
        vm.flush();
        assert_eq!(vm.get::<f64>("n"), 2.0);
        assert_eq!(vm.get::<f64>("added"), 1.0);
    }

    #[test]
    fn click_fires_click_and_activated() {
        let mut button = None;
        let vm = setup(|dom| {
            let r = dom.new_instance("TextButton", "B", None);
            button = Some(r);
            vec![("b", r)]
        });
        vm.run(
            "b.MouseButton1Click:Connect(function() clicked = true end) \
             b.Activated:Connect(function() activated = true end)",
        );
        let event = GuiEvent {
            referent: button.unwrap(),
            kind: GuiEventKind::Click {
                button: 1,
                x: 1.0,
                y: 2.0,
            },
        };
        fire_gui(&vm.lua, &event).unwrap();
        assert!(vm.get::<bool>("clicked"));
        assert!(vm.get::<bool>("activated"));
    }

    #[test]
    fn text_changed_writes_text() {
        let mut text_box = None;
        let vm = setup(|dom| {
            let r = dom.new_instance("TextBox", "T", None);
            text_box = Some(r);
            vec![("t", r)]
        });
        let r = text_box.unwrap();
        fire_gui(
            &vm.lua,
            &GuiEvent {
                referent: r,
                kind: GuiEventKind::TextChanged("typed".into()),
            },
        )
        .unwrap();
        let dom = vm.slot.borrow();
        assert_eq!(
            dom.get(r).unwrap().properties().get("Text"),
            Some(&Variant::String("typed".into()))
        );
    }

    #[test]
    fn handle_drag_passes_face_and_distance() {
        let mut handles = None;
        let vm = setup(|dom| {
            let r = dom.new_instance("Handles", "H", None);
            handles = Some(r);
            vec![("h", r)]
        });
        vm.run("h.MouseDrag:Connect(function(face, d) faceName = face.Name dist = d end)");
        let event = HandleEvent {
            referent: handles.unwrap(),
            kind: HandleEventKind::Drag {
                face: 1,
                distance: 3.5,
            },
        };
        fire_handle(&vm.lua, &event).unwrap();
        assert_eq!(vm.get::<String>("faceName"), "Top");
        assert_eq!(vm.get::<f64>("dist"), 3.5);
    }
}
