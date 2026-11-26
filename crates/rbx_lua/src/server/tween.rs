//! `TweenService` in Local Run: our own `tween.luau` keeps the tweens and
//! writes their properties once a frame ([`step`]); each `Tween` is a real
//! (unparented) instance whose `PlaybackState` lives in the DOM and whose
//! `Completed` is an instance signal.

use mlua::{Function, IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use crate::engine::engine;
use crate::engine::events::instance_key;
use crate::engine::signal::{self, LuaSignal};
use crate::instance::LuaInstance;

const SOURCE: &str = include_str!("tween.luau");
const KEY: &str = "rbxn.server.tween";

fn host(lua: &Lua) -> Result<Table> {
    let host = lua.create_table()?;
    host.set(
        "new",
        lua.create_function(|lua, target: mlua::AnyUserData| {
            let ctx = engine(lua).ctx.clone();
            let target = target.borrow::<LuaInstance>()?.referent();
            let tween = {
                let mut dom = ctx.dom_mut();
                let tween = dom.new_instance("Tween", "Tween", None);
                // Parented to nothing, as Instance.new's: not a root of the place.
                dom.unparent(tween);
                let _ = dom.set_property(tween, "Instance", Variant::Ref(target));
                let _ = dom.set_property(tween, "PlaybackState", Variant::Enum(0));
                tween
            };
            let object = LuaInstance::new(tween, ctx).into_lua(lua)?;
            Ok((object, tween.value()))
        })?,
    )?;
    host.set(
        "id",
        lua.create_function(|_, target: mlua::AnyUserData| {
            Ok(target.borrow::<LuaInstance>()?.referent().value())
        })?,
    )?;
    // A property's type as Roblox names it in Create's errors (`float`,
    // `double`, `int` for an enum), `nil` when there is no such property.
    host.set(
        "kind",
        lua.create_function(|lua, (target, name): (mlua::AnyUserData, String)| {
            let ctx = engine(lua).ctx.clone();
            let target = target.borrow::<LuaInstance>()?.referent();
            let dom = ctx.dom();
            let db = ctx.database();
            let Some(class) = dom.get(target).map(|i| i.class().to_owned()) else {
                return Ok(None);
            };
            Ok(db.resolve_property(&class, &name).map(|p| {
                if db.enum_items(&p.value_type).is_some() {
                    "int".to_owned()
                } else {
                    p.value_type.clone()
                }
            }))
        })?,
    )?;
    host.set(
        "setState",
        lua.create_function(|lua, (id, value): (u32, u32)| {
            let ctx = engine(lua).ctx.clone();
            let _ = ctx
                .dom_mut()
                .set_property(Ref::new(id), "PlaybackState", Variant::Enum(value));
            Ok(())
        })?,
    )?;
    host.set(
        "fire",
        lua.create_function(|lua, (id, state): (u32, Value)| {
            signal::fire_values(
                lua,
                &instance_key(Ref::new(id), "Completed"),
                MultiValue::from_iter([state]),
            )
        })?,
    )?;
    Ok(host)
}

fn module(lua: &Lua) -> Result<Table> {
    if let Some(module) = lua.named_registry_value::<Option<Table>>(KEY)? {
        return Ok(module);
    }
    let module: Table = lua
        .load(SOURCE)
        .set_name("=TweenService")
        .call(host(lua)?)?;
    lua.set_named_registry_value(KEY, module.clone())?;
    Ok(module)
}

/// `TweenService`'s members and a `Tween`'s (`Play`, `Pause`, `Cancel`,
/// `TweenInfo`, `Completed`).
pub(super) fn member(lua: &Lua, referent: Ref, class: &str, key: &str) -> Result<Option<Value>> {
    let (value, found): (Value, bool) = match class {
        "TweenService" => module(lua)?.get::<Function>("service")?.call(key)?,
        "Tween" if key == "Completed" => {
            let signal = LuaSignal(instance_key(referent, key));
            return lua
                .create_userdata(signal)
                .map(|u| Some(Value::UserData(u)));
        }
        "Tween" => module(lua)?
            .get::<Function>("tween")?
            .call((referent.value(), key))?,
        _ => return Ok(None),
    };
    Ok(found.then_some(value))
}

/// Steps every playing tween `dt` seconds.
pub(super) fn step(lua: &Lua, dt: f64) -> Result<()> {
    if lua.named_registry_value::<Option<Table>>(KEY)?.is_none() {
        return Ok(());
    }
    module(lua)?.get::<Function>("step")?.call(dt)
}
