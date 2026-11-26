//! A client's input in Local Run: `UserInputService`, `ContextActionService`
//! and `RunService:BindToRenderStep`, kept by our own `input.luau` and fed
//! one `InputObject` at a time by the simulation.

use mlua::{Function, IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_dom::Ref;

use crate::engine::engine;
use crate::engine::events::instance_key;
use crate::engine::signal::LuaSignal;
use crate::instance::LuaInstance;

use super::services::Device;

const SOURCE: &str = include_str!("input.luau");
const KEY: &str = "rbxn.server.input";

/// The `UserInputService` signals a script can connect to.
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
    "TouchTapInWorld",
    "TouchLongPress",
    "TouchPan",
    "TouchPinch",
    "TouchRotate",
    "TouchSwipe",
    "TouchDrag",
    "PointerAction",
    "DeviceAccelerationChanged",
    "DeviceGravityChanged",
    "DeviceRotationChanged",
];

fn module(lua: &Lua) -> Result<Table> {
    if let Some(module) = lua.named_registry_value::<Option<Table>>(KEY)? {
        return Ok(module);
    }
    let module: Table = lua.load(SOURCE).set_name("=InputService").eval()?;
    module.set(
        "newInput",
        lua.create_function(|lua, (kind, code): (String, String)| new_input(lua, &kind, &code))?,
    )?;
    lua.set_named_registry_value(KEY, module.clone())?;
    Ok(module)
}

/// An `InputObject` nothing has touched yet (a gamepad button's state, a
/// sensor before its first reading).
fn new_input(lua: &Lua, kind: &str, code: &str) -> Result<Value> {
    let ctx = engine(lua).ctx.clone();
    let db = ctx.database();
    let referent = ctx
        .dom_mut()
        .new_instance("InputObject", "InputObject", None);
    ctx.dom_mut().unparent(referent);
    for (property, enum_name, item) in [
        ("UserInputType", "UserInputType", kind),
        ("KeyCode", "KeyCode", code),
        ("UserInputState", "UserInputState", "None"),
    ] {
        if let Some(value) = db.enum_value(enum_name, item) {
            let _ = ctx
                .dom_mut()
                .set_property(referent, property, rbx_dom::Variant::Enum(value));
        }
    }
    LuaInstance::new(referent, ctx).into_lua(lua)
}

/// `UserInputService`'s and `ContextActionService`'s members, and
/// `RunService`'s render-step bindings (`service` names which).
pub(super) fn member(
    lua: &Lua,
    referent: Option<Ref>,
    service: &str,
    key: &str,
    device: Device,
) -> Result<Option<Value>> {
    if let Some(referent) = referent.filter(|_| SIGNALS.contains(&key)) {
        let signal = LuaSignal(instance_key(referent, key));
        return lua
            .create_userdata(signal)
            .map(|u| Some(Value::UserData(u)));
    }
    if service == "UserInputService" {
        let enabled = match key {
            "TouchEnabled" => Some(device.touch),
            "KeyboardEnabled" => Some(device.keyboard),
            "MouseEnabled" => Some(device.mouse),
            "GamepadEnabled" => Some(device.gamepad),
            _ => None,
        };
        if let Some(on) = enabled {
            return Ok(Some(Value::Boolean(on)));
        }
    }
    let module = module(lua)?;
    module.set("touch", device.touch)?;
    let (value, found): (Value, bool) = module.get::<Function>("member")?.call((service, key))?;
    Ok(found.then_some(value))
}

/// Starts a frame's input: the mouse delta restarts from zero.
pub(super) fn frame(lua: &Lua) -> Result<()> {
    module(lua)?.get::<Function>("frame")?.call(())
}

/// Hands `input` to the bound actions, unless the GUI `sunk` it: whether
/// it was taken, and whether it changed the last input type.
pub(super) fn dispatch(lua: &Lua, input: Ref, sunk: bool) -> Result<(bool, bool)> {
    let ctx = engine(lua).ctx.clone();
    let input = LuaInstance::new(input, ctx).into_lua(lua)?;
    module(lua)?
        .get::<Function>("dispatch")?
        .call((input, sunk))
}

/// What the touch or sensor signal `name` fires with after `input`'s own.
pub(super) fn also(lua: &Lua, name: &str, input: Ref, processed: bool) -> Result<MultiValue> {
    let input = LuaInstance::new(input, engine(lua).ctx.clone()).into_lua(lua)?;
    module(lua)?
        .get::<Function>("also")?
        .call((name, input, processed))
}

/// A gamepad plugged in or out; whether it changed.
pub(super) fn gamepad(lua: &Lua, pad: u8, connected: bool) -> Result<bool> {
    module(lua)?
        .get::<Function>("gamepad")?
        .call((pad, connected))
}

/// What `VirtualInput`s sent since the last call.
pub(super) fn take_virtual(lua: &Lua) -> Result<Vec<VirtualInput>> {
    let sent: Vec<Table> = module(lua)?.get::<Function>("takeVirtual")?.call(())?;
    sent.into_iter()
        .map(|t| {
            let kind: String = t.get(1)?;
            Ok(match kind.as_str() {
                "key" => VirtualInput::Key {
                    code: t.get(2)?,
                    down: t.get(3)?,
                },
                "button" => VirtualInput::Button {
                    button: t.get(2)?,
                    down: t.get(3)?,
                },
                "move" => VirtualInput::Move {
                    position: [t.get(2)?, t.get(3)?],
                    delta: [t.get(4)?, t.get(5)?],
                },
                "wheel" => VirtualInput::Wheel { notches: t.get(2)? },
                _ => VirtualInput::Text(t.get(2)?),
            })
        })
        .collect()
}

/// What a script's `VirtualInput` sent (`UserInputService:CreateVirtualInput`),
/// for the engine to play as the player's own input.
#[derive(Clone, Debug, PartialEq)]
pub enum VirtualInput {
    Key {
        code: String,
        down: bool,
    },
    Button {
        button: u8,
        down: bool,
    },
    /// Viewport pixels, top bar included.
    Move {
        position: [f32; 2],
        delta: [f32; 2],
    },
    Wheel {
        notches: f32,
    },
    Text(String),
}

/// Runs the render-step bindings in priority order.
pub(super) fn render(lua: &Lua, dt: f64) -> Result<()> {
    module(lua)?.get::<Function>("render")?.call(dt)
}
