//! Class members plugins commonly reach for that need no engine behind
//! them: `Players` name lookups, the legacy GUI tweens, translators, and
//! `LogService`.

use mlua::{Lua, MultiValue, Result, Table, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::plugin::signal::{LuaSignal, Owner, SignalKey};

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let is = |base: &str| ctx.database().is_subclass_of(class, base);
    if class == "Players" {
        return crate::plugin::services::players_member(lua, key);
    }
    if is("GuiObject") {
        if let Some(fields) = tween_fields(key) {
            return gui_tween(lua, fields).map(Some);
        }
    }
    match (class, key) {
        ("LocalizationTable", "GetTranslator")
        | ("LocalizationService", "GetTranslatorForLocaleAsync")
        | ("LocalizationService", "GetTranslatorForPlayerAsync")
        | ("LocalizationService", "GetTranslatorForPlayer") => {
            let translator = translator(lua)?;
            Ok(Some(Value::Function(lua.create_function(
                move |_, _: MultiValue| Ok(translator.clone()),
            )?)))
        }
        ("LocalizationService", "RobloxLocaleId" | "SystemLocaleId") => {
            Ok(Some(Value::String(lua.create_string("en-us")?)))
        }
        ("LogService", "MessageOut") => {
            let signal = LuaSignal(SignalKey::new(Owner::Instance(referent), key));
            Ok(Some(Value::UserData(lua.create_userdata(signal)?)))
        }
        ("LogService", "GetLogHistory") => Ok(Some(Value::Function(
            lua.create_function(|lua, _: MultiValue| lua.create_table())?,
        ))),
        _ => Ok(None),
    }
}

/// Which properties `TweenPosition`/`TweenSize`/`TweenSizeAndPosition`
/// set, in argument order.
fn tween_fields(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "TweenPosition" => Some(&["Position"]),
        "TweenSize" => Some(&["Size"]),
        "TweenSizeAndPosition" => Some(&["Size", "Position"]),
        _ => None,
    }
}

/// The deprecated GUI tweens, finished at once: the goal is set, the
/// callback (last argument when a function) gets
/// `Enum.TweenStatus.Completed`, and the call returns `true`.
fn gui_tween(lua: &Lua, fields: &'static [&'static str]) -> Result<Value> {
    let f = lua.create_function(move |lua, args: MultiValue| {
        let mut args = args.into_iter();
        let this = args.next().unwrap_or(Value::Nil);
        let Value::UserData(object) = &this else {
            return Err(mlua::Error::runtime("Expected ':' not '.' calling a tween"));
        };
        let rest: Vec<Value> = args.collect();
        for (field, goal) in fields.iter().zip(rest.iter()) {
            mlua::ObjectLike::set(object, *field, goal.clone())?;
        }
        if let Some(Value::Function(callback)) = rest.iter().rev().find(|v| v.is_function()) {
            let completed: Value = lua.load("return Enum.TweenStatus.Completed").eval()?;
            callback.call::<()>(completed)?;
        }
        Ok(true)
    })?;
    Ok(Value::Function(f))
}

/// A `Translator` that hands text back untranslated.
fn translator(lua: &Lua) -> Result<Value> {
    const KEY: &str = "rbxn.translator";
    if let Some(found) = lua.named_registry_value::<Option<Value>>(KEY)? {
        return Ok(found);
    }
    let made: Table = lua
        .load(
            r#"
            return {
                LocaleId = "en-us",
                Translate = function(_, _, text) return text end,
                FormatByKey = function(_, key) return key end,
            }
            "#,
        )
        .set_name("=Translator")
        .eval()?;
    let value = Value::Table(made);
    lua.set_named_registry_value(KEY, value.clone())?;
    Ok(value)
}
