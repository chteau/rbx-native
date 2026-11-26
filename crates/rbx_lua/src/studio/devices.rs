//! `StudioDeviceSimulatorService`: the device emulator's state, with
//! Studio 0.748's device list (`devices.tsv`, read from its Command Bar
//! with `GetDeviceInfoAsync` for every id `GetDeviceListAsync` gives).
//!
//! Nothing is emulated until `SetDeviceAsync`; `StopSimulationAsync` goes
//! back to "default", as in Studio.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, MultiValue, Result, Table, Value};
use rbx_dom::Ref;

use super::services::enum_item;
use crate::ctx::Ctx;
use crate::engine::signal::{self, Owner, SignalKey};
use crate::lua_enum::LuaEnumItem;

const DEVICES: &str = include_str!("devices.tsv");
const NO_DEVICE: &str =
    "StudioDeviceSimulatorService: no device is active — call SetDeviceAsync() first";

#[derive(Clone)]
struct Device {
    id: String,
    name: String,
    size: (f64, f64),
    density: f64,
    scale: f64,
    keyboards: (f64, f64),
    form: Option<String>,
    custom: bool,
}

#[derive(Default)]
struct State {
    custom: Vec<Device>,
    next_custom: u32,
    /// The emulated device and what was changed on it since.
    active: Option<Device>,
    orientation: Option<String>,
    scaling: Option<String>,
}

fn state(lua: &Lua) -> Rc<RefCell<State>> {
    if let Some(found) = lua.app_data_ref::<Rc<RefCell<State>>>() {
        return found.clone();
    }
    let made = Rc::new(RefCell::new(State::default()));
    lua.set_app_data(made.clone());
    made
}

fn builtin() -> Vec<Device> {
    DEVICES
        .lines()
        .filter_map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            let n = |i: usize| c.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            Some(Device {
                id: c.first()?.to_string(),
                name: c.get(1)?.to_string(),
                size: (n(2), n(3)),
                density: n(4),
                scale: n(5),
                keyboards: (n(6), n(7)),
                form: c.get(8).filter(|f| !f.is_empty()).map(|f| f.to_string()),
                custom: false,
            })
        })
        .collect()
}

fn find(lua: &Lua, id: &str) -> Option<Device> {
    builtin()
        .into_iter()
        .chain(state(lua).borrow().custom.iter().cloned())
        .find(|d| d.id == id)
}

fn info(lua: &Lua, ctx: &Ctx, device: &Device) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("DeviceId", device.id.as_str())?;
    t.set("Name", device.name.as_str())?;
    t.set("Width", device.size.0)?;
    t.set("Height", device.size.1)?;
    t.set("PixelDensity", device.density)?;
    t.set("ResolutionScale", device.scale)?;
    t.set("PortraitKeyboardHeight", device.keyboards.0)?;
    t.set("LandscapeKeyboardHeight", device.keyboards.1)?;
    t.set("IsCustom", device.custom)?;
    if let Some(form) = &device.form {
        t.set("DeviceForm", enum_item(lua, ctx, "DeviceForm", form)?)?;
    }
    Ok(t)
}

fn string_arg(args: &MultiValue, n: usize) -> String {
    args.get(n + 1)
        .and_then(|v| v.as_string().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default()
}

fn number_arg(args: &MultiValue, n: usize) -> f64 {
    args.get(n + 1)
        .and_then(|v| v.as_f64().or(v.as_i64().map(|i| i as f64)))
        .unwrap_or(0.0)
}

fn enum_arg(args: &MultiValue, n: usize) -> Option<String> {
    args.get(n + 1).and_then(|v| v.as_userdata()).and_then(|u| {
        u.borrow::<LuaEnumItem>()
            .ok()
            .map(|i| i.item_name().to_owned())
    })
}

/// A device from a config table, filling in from `base`.
fn from_config(config: &Value, base: Device) -> Result<Device> {
    let Some(t) = config.as_table() else {
        return Ok(base);
    };
    let n = |key: &str, old: f64| -> Result<f64> { Ok(t.get::<Option<f64>>(key)?.unwrap_or(old)) };
    Ok(Device {
        name: t
            .get::<Option<String>>("Name")?
            .unwrap_or(base.name.clone()),
        size: (n("Width", base.size.0)?, n("Height", base.size.1)?),
        density: n("PixelDensity", base.density)?,
        scale: n("ResolutionScale", base.scale)?,
        keyboards: (
            n("PortraitKeyboardHeight", base.keyboards.0)?,
            n("LandscapeKeyboardHeight", base.keyboards.1)?,
        ),
        ..base
    })
}

fn changed(lua: &Lua, referent: Ref) -> Result<()> {
    signal::fire(
        lua,
        &SignalKey::new(Owner::Instance(referent), "ConfigurationChanged"),
        &[],
    )
}

fn fail(message: String) -> mlua::Error {
    mlua::Error::runtime(message)
}

fn active(lua: &Lua) -> Result<Device> {
    state(lua)
        .borrow()
        .active
        .clone()
        .ok_or_else(|| fail(NO_DEVICE.to_owned()))
}

pub(super) fn member(lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match key {
        "GetDeviceListAsync" => lua.create_function(|lua, _: MultiValue| {
            let ids: Vec<String> = builtin()
                .into_iter()
                .chain(state(lua).borrow().custom.iter().cloned())
                .map(|d| d.id)
                .collect();
            lua.create_sequence_from(ids)
        })?,
        "GetDeviceAsync" => lua.create_function(|lua, _: MultiValue| {
            Ok(state(lua)
                .borrow()
                .active
                .as_ref()
                .map_or("default".to_owned(), |d| d.id.clone()))
        })?,
        "GetDeviceInfoAsync" => lua.create_function(move |lua, args: MultiValue| {
            let id = string_arg(&args, 0);
            let device = find(lua, &id).ok_or_else(|| {
                fail(format!(
                    "StudioDeviceSimulatorService: device not found: {id}"
                ))
            })?;
            info(lua, &ctx, &device)
        })?,
        "SetDeviceAsync" => lua.create_function(move |lua, args: MultiValue| {
            let id = string_arg(&args, 0);
            let device = find(lua, &id).ok_or_else(|| {
                fail(format!(
                    "StudioDeviceSimulatorService: device not found: {id}"
                ))
            })?;
            state(lua).borrow_mut().active = Some(device);
            changed(lua, referent)
        })?,
        "StopSimulationAsync" => lua.create_function(move |lua, _: MultiValue| {
            let state = state(lua);
            let mut s = state.borrow_mut();
            s.active = None;
            s.orientation = None;
            s.scaling = None;
            drop(s);
            changed(lua, referent)
        })?,
        "GetOrientationAsync" => lua.create_function(move |lua, _: MultiValue| {
            let orientation = state(lua).borrow().orientation.clone();
            enum_item(
                lua,
                &ctx,
                "ScreenOrientation",
                orientation.as_deref().unwrap_or("LandscapeRight"),
            )
        })?,
        "SetOrientationAsync" => lua.create_function(move |lua, args: MultiValue| {
            let orientation =
                enum_arg(&args, 0).ok_or_else(|| fail("Argument 1 missing or nil".into()))?;
            state(lua).borrow_mut().orientation = Some(orientation);
            changed(lua, referent)
        })?,
        "GetPixelDensityAsync" => {
            lua.create_function(|lua, _: MultiValue| Ok(active(lua)?.density))?
        }
        "GetResolutionAsync" => lua.create_function(|lua, _: MultiValue| {
            let (x, y) = active(lua)?.size;
            lua.load("return Vector2.new(...)").call::<Value>((x, y))
        })?,
        "GetScalingModeAsync" => lua.create_function(move |lua, _: MultiValue| {
            active(lua)?;
            let scaling = state(lua).borrow().scaling.clone();
            enum_item(
                lua,
                &ctx,
                "DeviceSimulatorScalingMode",
                scaling.as_deref().unwrap_or("ScaleToPhysicalSize"),
            )
        })?,
        "SetPixelDensityAsync" | "SetResolutionAsync" | "SetScalingModeAsync" => {
            let which = key.to_owned();
            lua.create_function(move |lua, args: MultiValue| {
                let mut device = active(lua)?;
                match which.as_str() {
                    "SetPixelDensityAsync" => device.density = number_arg(&args, 0),
                    "SetResolutionAsync" => {
                        device.size = (number_arg(&args, 0), number_arg(&args, 1))
                    }
                    _ => state(lua).borrow_mut().scaling = enum_arg(&args, 0),
                }
                state(lua).borrow_mut().active = Some(device);
                changed(lua, referent)
            })?
        }
        "CreateDeviceAsync" => lua.create_function(|lua, args: MultiValue| {
            let state = state(lua);
            let mut s = state.borrow_mut();
            s.next_custom += 1;
            let id = format!("custom_{}", s.next_custom);
            let base = Device {
                id: id.clone(),
                name: id.clone(),
                size: (1920.0, 1080.0),
                density: 96.0,
                scale: 1.0,
                keyboards: (0.0, 0.0),
                form: None,
                custom: true,
            };
            let device = from_config(args.get(1).unwrap_or(&Value::Nil), base)?;
            s.custom.push(device);
            Ok(id)
        })?,
        "UpdateDeviceAsync" => lua.create_function(move |lua, args: MultiValue| {
            let id = string_arg(&args, 0);
            let state = state(lua);
            let mut s = state.borrow_mut();
            let Some(slot) = s.custom.iter_mut().find(|d| d.id == id) else {
                return Err(fail(format!(
                    "StudioDeviceSimulatorService: device not found: {id}"
                )));
            };
            *slot = from_config(args.get(2).unwrap_or(&Value::Nil), slot.clone())?;
            drop(s);
            changed(lua, referent)
        })?,
        "RemoveDeviceAsync" => lua.create_function(|lua, args: MultiValue| {
            let id = string_arg(&args, 0);
            let state = state(lua);
            let mut s = state.borrow_mut();
            let before = s.custom.len();
            s.custom.retain(|d| d.id != id);
            if s.custom.len() == before {
                return Err(fail(format!(
                    "StudioDeviceSimulatorService: device not found: {id}"
                )));
            }
            Ok(())
        })?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
