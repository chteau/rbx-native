//! Instance events and the plugin-only members of instances, through
//! `crate::hooks`.

use std::collections::{BTreeMap, HashMap};

use mlua::{Function, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_dom::{Color3Data, Vector2Data, Vector3Data};
use rbx_reflection::ReflectionDatabase;

use super::api::{Arg, GuiEvent, GuiEventKind, HandleEvent, HandleEventKind, OutputKind};
use super::arg;
use super::env::env;
use super::signal::{self, LuaSignal, Owner, SignalKey};
use crate::ctx::Ctx;
use crate::datatypes::misc::LuaVector2;
use crate::hooks::Hooks;

/// Where the place files keep the attribute blob.
const ATTRIBUTES: &str = "AttributesSerialize";
/// The binary format's string type id, which the attribute blob travels as.
const STRING_TYPE_ID: u8 = 0x01;
const ON_INVOKE_KEY: &str = "__plugin_on_invoke";
const BIND_TO_CLOSE_KEY: &str = "__plugin_bind_to_close";

const INSTANCE_SIGNALS: &[&str] = &[
    "Changed",
    "ChildAdded",
    "ChildRemoved",
    "DescendantAdded",
    "DescendantRemoving",
    "AncestryChanged",
    "Destroying",
    "AttributeChanged",
];
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
/// `index` answers, by class (`Ctx::database().is_subclass_of`):
/// - every instance: `Changed`, `ChildAdded`, `ChildRemoved`,
///   `DescendantAdded`, `DescendantRemoving`, `AncestryChanged`,
///   `Destroying`, `AttributeChanged` signals; `GetPropertyChangedSignal(
///   name)` and `GetAttributeChangedSignal(name)` functions;
///   `GetAttribute`, `SetAttribute`, `GetAttributes` (over the
///   `Attributes` property via `rbx_dom::attributes`);
///   `WaitForChild(name, timeout?)` (the child, or nil after a warning —
///   nothing loads later in the editor);
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
/// - `BindableEvent`: `Event`, `Fire(...)`; `BindableFunction`:
///   `Invoke(...)` (calls `OnInvoke` synchronously);
/// - `PluginGui` / `DockWidgetPluginGui` roots: `BindToClose(fn?)`,
///   `WindowFocused`, `WindowFocusReleased`, `GetRelativeMousePosition()`,
///   `HostWidgetWasRestored` (false).
///
/// `new_index` handles `OnInvoke` on a `BindableFunction` (stored in the
/// Lua registry), and for a widget root keeps `Ui` in step with `Title`
/// and `Enabled` (returning false so the property is written too).
/// `service` delegates to `services::service`.
pub(crate) struct PluginHooks;

fn is_handles(db: &ReflectionDatabase, class: &str) -> bool {
    matches!(class, "Handles" | "ArcHandles") || db.is_subclass_of(class, "HandlesBase")
}

fn is_signal(db: &ReflectionDatabase, class: &str, key: &str) -> bool {
    let has = |list: &[&str], base: &str| list.contains(&key) && db.is_subclass_of(class, base);
    INSTANCE_SIGNALS.contains(&key)
        || has(GUI_OBJECT_SIGNALS, "GuiObject")
        || has(GUI_BUTTON_SIGNALS, "GuiButton")
        || has(TEXT_BOX_SIGNALS, "TextBox")
        || (HANDLES_SIGNALS.contains(&key) && is_handles(db, class))
        || has(HANDLE_ADORNMENT_SIGNALS, "HandleAdornment")
        || has(PLUGIN_GUI_SIGNALS, "PluginGui")
        || has(&["Event"], "BindableEvent")
        || has(&["OnServerEvent", "OnClientEvent"], "RemoteEvent")
}

fn instance_key(referent: Ref, event: impl Into<String>) -> SignalKey {
    SignalKey::new(Owner::Instance(referent), event)
}

fn registry_table(lua: &Lua, name: &str) -> Result<Table> {
    if let Some(table) = lua.named_registry_value::<Option<Table>>(name)? {
        return Ok(table);
    }
    let table = lua.create_table()?;
    lua.set_named_registry_value(name, table.clone())?;
    Ok(table)
}

fn ref_key(referent: Ref) -> String {
    format!("{referent:?}")
}

fn read_attributes(ctx: &Ctx, referent: Ref) -> BTreeMap<String, Variant> {
    let dom = ctx.dom();
    let blob = dom
        .get(referent)
        .and_then(|i| i.properties().get(ATTRIBUTES));
    rbx_dom::attributes::decode(blob)
}

fn variant_to_arg(variant: &Variant) -> Arg {
    match variant {
        Variant::String(s) => Arg::String(s.clone()),
        Variant::Bool(b) => Arg::Bool(*b),
        Variant::Int32(n) => Arg::Number(f64::from(*n)),
        Variant::Int64(n) => Arg::Number(*n as f64),
        Variant::Float32(n) => Arg::Number(f64::from(*n)),
        Variant::Float64(n) => Arg::Number(*n),
        Variant::Vector2(v) => Arg::Vector2([v.x, v.y]),
        Variant::Vector3(v) => Arg::Vector3([v.x, v.y, v.z]),
        Variant::Color3(c) => Arg::Color3([c.r, c.g, c.b]),
        Variant::BrickColor(n) => Arg::BrickColor(*n),
        _ => Arg::Nil,
    }
}

fn arg_to_variant(name: &str, value: Arg) -> Result<Option<Variant>> {
    Ok(Some(match value {
        Arg::Nil => return Ok(None),
        Arg::String(s) => Variant::String(s),
        Arg::Bool(b) => Variant::Bool(b),
        Arg::Number(n) => Variant::Float64(n),
        Arg::Vector2([x, y]) => Variant::Vector2(Vector2Data { x, y }),
        Arg::Vector3([x, y, z]) => Variant::Vector3(Vector3Data { x, y, z }),
        Arg::Color3([r, g, b]) => Variant::Color3(Color3Data { r, g, b }),
        Arg::BrickColor(n) => Variant::BrickColor(n),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "attribute '{name}' cannot hold a value of this type"
            )))
        }
    }))
}

fn set_attribute(ctx: &Ctx, referent: Ref, name: &str, value: Option<Variant>) -> Result<()> {
    ctx.check_write(referent)?;
    let mut attributes = read_attributes(ctx, referent);
    match value {
        Some(value) => attributes.insert(name.to_string(), value),
        None => attributes.remove(name),
    };
    let raw = rbx_dom::attributes::encode(&attributes, |_| None)
        .ok_or_else(|| mlua::Error::runtime("these attributes cannot be written back"))?;
    ctx.dom_mut()
        .set_property(
            referent,
            ATTRIBUTES,
            Variant::Unknown {
                type_id: STRING_TYPE_ID,
                raw,
            },
        )
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
    Ok(())
}

fn signal_getter(lua: &Lua, referent: Ref, prefix: &'static str) -> Result<Value> {
    lua.create_function(move |_, (_this, name): (Value, String)| {
        Ok(LuaSignal(instance_key(
            referent,
            format!("{prefix}:{name}"),
        )))
    })
    .map(Value::Function)
}

fn instance_function(lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match key {
        "GetPropertyChangedSignal" => return signal_getter(lua, referent, "Changed").map(Some),
        "GetAttributeChangedSignal" => return signal_getter(lua, referent, "AttributeChanged").map(Some),
        "GetAttribute" => lua.create_function(move |lua, (_this, name): (Value, String)| {
            let value = read_attributes(&ctx, referent).get(&name).map(variant_to_arg);
            arg::to_lua(lua, &ctx, &value.unwrap_or(Arg::Nil))
        })?,
        "SetAttribute" => lua.create_function(move |_, (_this, name, value): (Value, String, Value)| {
            let variant = arg_to_variant(&name, arg::from_lua(&value))?;
            set_attribute(&ctx, referent, &name, variant)
        })?,
        "GetAttributes" => lua.create_function(move |lua, _this: Value| {
            let table = lua.create_table()?;
            for (name, value) in read_attributes(&ctx, referent) {
                table.set(name, arg::to_lua(lua, &ctx, &variant_to_arg(&value))?)?;
            }
            Ok(table)
        })?,
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
        "Fire" if is("BindableEvent") => {
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                signal::fire_values(lua, &instance_key(referent, "Event"), args)
            })?
        }
        "Invoke" if is("BindableFunction") => {
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let callback: Option<Function> =
                    registry_table(lua, ON_INVOKE_KEY)?.get(ref_key(referent))?;
                match callback {
                    Some(callback) => callback.call::<MultiValue>(args),
                    None => Err(mlua::Error::runtime(
                        "OnInvoke is not set on this BindableFunction",
                    )),
                }
            })?
        }
        // Inside a plugin there is no network: a remote is a bindable with
        // a player argument. `…Server` calls pass `nil` for the player the
        // server would see; `…Client` calls drop the player they name.
        "FireServer" if is("RemoteEvent") => {
            lua.create_function(move |lua, (_this, mut args): (Value, MultiValue)| {
                args.push_front(Value::Nil);
                signal::fire_values(lua, &instance_key(referent, "OnServerEvent"), args)
            })?
        }
        "FireClient" | "FireAllClients" if is("RemoteEvent") => {
            let skip = usize::from(key == "FireClient");
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let args: MultiValue = args.into_iter().skip(skip).collect();
                signal::fire_values(lua, &instance_key(referent, "OnClientEvent"), args)
            })?
        }
        "InvokeServer" | "InvokeClient" if is("RemoteFunction") => {
            let server = key == "InvokeServer";
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let side = if server { "server" } else { "client" };
                let callback: Option<Function> = registry_table(lua, ON_INVOKE_KEY)?
                    .get(format!("{side}:{}", ref_key(referent)))?;
                let mut args: MultiValue = args.into_iter().skip(usize::from(!server)).collect();
                if server {
                    args.push_front(Value::Nil);
                }
                match callback {
                    Some(callback) => callback.call::<MultiValue>(args),
                    None => Err(mlua::Error::runtime(format!(
                        "On{}Invoke is not set on this RemoteFunction",
                        if server { "Server" } else { "Client" }
                    ))),
                }
            })?
        }
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

/// `game`'s own signals, keyed under `Owner::Service("DataModel")`;
/// `DescendantAdded`/`DescendantRemoving` fire for the place's instances
/// (see `fire_parent`), the others are there to connect to.
fn game_signal(event: impl Into<String>) -> LuaSignal {
    LuaSignal(SignalKey::new(Owner::Service("DataModel"), event))
}

impl Hooks for PluginHooks {
    fn game_member(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        Ok(Some(match key {
            "Changed" | "ChildAdded" | "ChildRemoved" | "DescendantAdded"
            | "DescendantRemoving" | "ItemChanged" | "Close" | "AncestryChanged" => {
                Value::UserData(lua.create_userdata(game_signal(key))?)
            }
            "GetPropertyChangedSignal" => {
                Value::Function(lua.create_function(|_, (_this, name): (Value, String)| {
                    Ok(game_signal(format!("Changed:{name}")))
                })?)
            }
            _ => return Ok(None),
        }))
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
        let db = ctx.database();
        if is_signal(db, class, key) {
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
        let db = ctx.database();
        if key == "OnInvoke" && db.is_subclass_of(class, "BindableFunction") {
            let callback = match value {
                Value::Function(f) => Some(f.clone()),
                Value::Nil => None,
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "OnInvoke must be a function, got {}",
                        other.type_name()
                    )))
                }
            };
            registry_table(lua, ON_INVOKE_KEY)?.set(ref_key(referent), callback)?;
            return Ok(true);
        }
        if matches!(key, "OnServerInvoke" | "OnClientInvoke")
            && db.is_subclass_of(class, "RemoteFunction")
        {
            let side = if key == "OnServerInvoke" {
                "server"
            } else {
                "client"
            };
            let callback = match value {
                Value::Function(f) => Some(f.clone()),
                _ => None,
            };
            registry_table(lua, ON_INVOKE_KEY)?
                .set(format!("{side}:{}", ref_key(referent)), callback)?;
            return Ok(true);
        }
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

/// `start` and every ancestor above it, nearest first.
fn chain(dom: &WeakDom, start: Option<Ref>) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut at = start;
    while let Some(r) = at {
        if dom.get(r).is_none() {
            break;
        }
        out.push(r);
        at = dom.parent(r);
    }
    out
}

/// `root` and everything under it.
fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        if let Some(instance) = dom.get(out[i]) {
            out.extend_from_slice(instance.children());
        }
        i += 1;
    }
    out
}

fn emit(lua: &Lua, referent: Ref, event: &str, args: &[Arg]) -> Result<()> {
    signal::fire(lua, &instance_key(referent, event), args)
}

fn fire_parent(lua: &Lua, referent: Ref, old: Option<Ref>, new: Option<Ref>) -> Result<()> {
    let (moved, old_chain, new_chain) = {
        let dom = lua_ctx_dom(lua);
        let dom = dom.dom();
        (subtree(&dom, referent), chain(&dom, old), chain(&dom, new))
    };
    let parent = new.map_or(Arg::Nil, Arg::Instance);
    for r in moved {
        emit(
            lua,
            r,
            "AncestryChanged",
            &[Arg::Instance(referent), parent.clone()],
        )?;
    }
    let child = [Arg::Instance(referent)];
    if let Some(old) = old {
        emit(lua, old, "ChildRemoved", &child)?;
    }
    if !old_chain.is_empty() && in_place(lua, &old_chain) {
        signal::fire(lua, &game_signal("DescendantRemoving").0, &child)?;
    }
    for r in old_chain {
        emit(lua, r, "DescendantRemoving", &child)?;
    }
    if let Some(new) = new {
        emit(lua, new, "ChildAdded", &child)?;
    }
    if !new_chain.is_empty() && in_place(lua, &new_chain) {
        signal::fire(lua, &game_signal("DescendantAdded").0, &child)?;
    }
    for r in new_chain {
        emit(lua, r, "DescendantAdded", &child)?;
    }
    Ok(())
}

/// Whether an ancestor chain (nearest first) ends at one of the place's
/// services rather than at the plugin's own parked instances.
fn in_place(lua: &Lua, chain: &[Ref]) -> bool {
    let Some(&top) = chain.last() else {
        return false;
    };
    let ctx = lua_ctx_dom(lua);
    let service = ctx
        .dom()
        .get(top)
        .is_some_and(|i| ctx.database().is_service(i.class()));
    service && !env(lua).roots.borrow().contains(&top)
}

fn lua_ctx_dom(lua: &Lua) -> Ctx {
    env(lua).ctx.clone()
}

/// Fires this VM's instance signals for a change log, reading the DOM as it
/// is now: `Property` → `Changed(name)` and `Changed:<name>` (and
/// `AttributeChanged`/`AttributeChanged:<n>` for `Attributes`, comparing
/// nothing — every attribute name present fires); `Parent` → the child's
/// `AncestryChanged(child, parent)` (and its descendants'), the old
/// parent's `ChildRemoved`, the new one's `ChildAdded`, and
/// `DescendantRemoving`/`DescendantAdded` up both ancestor chains;
/// `Added` with a parent → as a `Parent` change from nil; `Removed` →
/// `Destroying` on the instance. Only connected signals cost anything.
pub(crate) fn fire_changes(lua: &Lua, changes: &[Change]) -> Result<()> {
    let ctx = lua_ctx_dom(lua);
    for change in changes {
        match change {
            Change::Property { referent, name } => {
                // A value object's `Changed` passes the new `Value` instead
                // of a property name, and fires for nothing else.
                let value_object = ctx
                    .dom()
                    .get(*referent)
                    .is_some_and(|i| ctx.database().is_subclass_of(i.class(), "ValueBase"));
                if value_object {
                    if name == "Value" {
                        let value = crate::property::get(lua, &ctx, *referent, "Value")?
                            .unwrap_or(Value::Nil);
                        signal::fire_values(
                            lua,
                            &instance_key(*referent, "Changed"),
                            MultiValue::from_vec(vec![value]),
                        )?;
                    }
                } else {
                    emit(lua, *referent, "Changed", &[Arg::String(name.clone())])?;
                }
                emit(lua, *referent, &format!("Changed:{name}"), &[])?;
                if name == ATTRIBUTES || name == "Attributes" {
                    for attribute in read_attributes(&ctx, *referent).into_keys() {
                        emit(
                            lua,
                            *referent,
                            "AttributeChanged",
                            &[Arg::String(attribute.clone())],
                        )?;
                        emit(
                            lua,
                            *referent,
                            &format!("AttributeChanged:{attribute}"),
                            &[],
                        )?;
                    }
                }
            }
            Change::Parent { referent, old, new } => fire_parent(lua, *referent, *old, *new)?,
            Change::Added(referent) => {
                let parent = ctx.dom().parent(*referent);
                if parent.is_some() {
                    fire_parent(lua, *referent, None, parent)?;
                }
            }
            Change::Removed(referent) => emit(lua, *referent, "Destroying", &[])?,
            _ => {}
        }
    }
    Ok(())
}

/// Remembers the pointer for `referent` and for the widget root above it.
fn remember_pointer(lua: &Lua, referent: Ref, x: f32, y: f32) {
    let env = env(lua);
    let ancestors = chain(&env.ctx.dom(), Some(referent));
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
    let ctx = lua_ctx_dom(lua);
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

    use super::*;
    use crate::instance::LuaInstance;
    use crate::plugin::scheduler;
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
            let changes = self.slot.borrow_mut().take_changes();
            fire_changes(&self.lua, &changes).unwrap();
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
