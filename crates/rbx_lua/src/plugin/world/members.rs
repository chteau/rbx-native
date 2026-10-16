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
        ("LocalizationTable", "GetTranslator") => {
            let entries = table_entries(ctx, referent);
            let f = lua.create_function(move |lua, (_, locale): (Value, Option<String>)| {
                table_translator(lua, &entries, locale.as_deref().unwrap_or("en-us"))
            })?;
            Ok(Some(Value::Function(f)))
        }
        ("LocalizationService", "GetTranslatorForLocaleAsync")
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

/// A `LocalizationTable`'s entries, from the JSON its `Contents` holds:
/// `[{ "key", "source", "context", "values": { locale: text } }]`.
fn table_entries(ctx: &Ctx, referent: Ref) -> Vec<serde_json::Value> {
    let dom = ctx.dom();
    let text = dom
        .get(referent)
        .and_then(|i| match i.properties().get("Contents") {
            Some(rbx_dom::Variant::String(text)) => Some(text.clone()),
            _ => None,
        });
    text.and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok())
        .unwrap_or_default()
}

/// A `Translator` over one table's entries for `locale` (falling back to
/// `en-us`, then the source text): `FormatByKey(key, args)` with `{name}`
/// / `{1}` placeholders filled, `Translate(context, source)`.
fn table_translator(lua: &Lua, entries: &[serde_json::Value], locale: &str) -> Result<Value> {
    let by_key = lua.create_table()?;
    let by_source = lua.create_table()?;
    for entry in entries {
        let values = &entry["values"];
        let text = values[locale]
            .as_str()
            .or_else(|| values["en-us"].as_str())
            .or_else(|| entry["source"].as_str())
            .unwrap_or_default();
        if let Some(key) = entry["key"].as_str().filter(|k| !k.is_empty()) {
            by_key.set(key, text)?;
        }
        if let Some(source) = entry["source"].as_str().filter(|s| !s.is_empty()) {
            by_source.set(source, text)?;
        }
    }
    lua.load(
        r#"
        local byKey, bySource, locale = ...
        local function fill(text, args)
            if type(args) ~= "table" then return text end
            return (text:gsub("{([%w_]+)}", function(name)
                local value = args[name] or args[tonumber(name)]
                return value ~= nil and tostring(value) or nil
            end))
        end
        return {
            LocaleId = locale,
            FormatByKey = function(_, key, args)
                local text = byKey[key]
                if text == nil then error("FormatByKey: no entry for key " .. tostring(key), 2) end
                return fill(text, args)
            end,
            Translate = function(_, _, source) return bySource[source] or source end,
        }
        "#,
    )
    .set_name("=Translator")
    .call((by_key, by_source, locale))
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
