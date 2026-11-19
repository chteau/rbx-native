//! A client's input in Local Run: `UserInputService`, `ContextActionService`
//! and `RunService:BindToRenderStep`, kept by our own `input.luau` and fed
//! one `InputObject` at a time by the simulation.

use mlua::{Function, IntoLua, Lua, Result, Table, Value};
use rbx_dom::Ref;

use crate::engine::engine;
use crate::engine::events::instance_key;
use crate::engine::signal::LuaSignal;
use crate::instance::LuaInstance;

const SOURCE: &str = include_str!("input.luau");
const KEY: &str = "rbxn.server.input";

/// The `UserInputService` signals a script can connect to. Only the input
/// ones and `JumpRequest` ever fire (no touch, gamepad or text boxes yet).
const SIGNALS: &[&str] = &[
    "InputBegan",
    "InputChanged",
    "InputEnded",
    "JumpRequest",
    "LastInputTypeChanged",
    "WindowFocused",
    "WindowFocusReleased",
    "TextBoxFocused",
    "TextBoxFocusReleased",
    "GamepadConnected",
    "GamepadDisconnected",
    "TouchStarted",
    "TouchMoved",
    "TouchEnded",
    "TouchTap",
    "PointerAction",
];

fn module(lua: &Lua) -> Result<Table> {
    if let Some(module) = lua.named_registry_value::<Option<Table>>(KEY)? {
        return Ok(module);
    }
    let module: Table = lua.load(SOURCE).set_name("=InputService").eval()?;
    lua.set_named_registry_value(KEY, module.clone())?;
    Ok(module)
}

/// `UserInputService`'s and `ContextActionService`'s members, and
/// `RunService`'s render-step bindings (`service` names which).
pub(super) fn member(
    lua: &Lua,
    referent: Option<Ref>,
    service: &str,
    key: &str,
) -> Result<Option<Value>> {
    if let Some(referent) = referent.filter(|_| SIGNALS.contains(&key)) {
        let signal = LuaSignal(instance_key(referent, key));
        return lua
            .create_userdata(signal)
            .map(|u| Some(Value::UserData(u)));
    }
    let (value, found): (Value, bool) = module(lua)?
        .get::<Function>("member")?
        .call((service, key))?;
    Ok(found.then_some(value))
}

/// Starts a frame's input: the mouse delta restarts from zero.
pub(super) fn frame(lua: &Lua) -> Result<()> {
    module(lua)?.get::<Function>("frame")?.call(())
}

/// Hands `input` to the bound actions; whether one sank it.
pub(super) fn dispatch(lua: &Lua, input: Ref) -> Result<bool> {
    let ctx = engine(lua).ctx.clone();
    let input = LuaInstance::new(input, ctx).into_lua(lua)?;
    module(lua)?.get::<Function>("dispatch")?.call(input)
}

/// Runs the render-step bindings in priority order.
pub(super) fn render(lua: &Lua, dt: f64) -> Result<()> {
    module(lua)?.get::<Function>("render")?.call(dt)
}
