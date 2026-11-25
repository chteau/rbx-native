//! `ScriptDocument` and the `ScriptEditorService` members that hand them
//! out: one document per script tab the editor has open, plus the Command
//! Bar's, which Studio always lists. A script's text is its `Source` (the
//! editor keeps no other copy), so reads and edits go through the DOM.
//! Lines and characters count from 1, end positions are exclusive, as in
//! Studio.

use mlua::{
    AnyUserData, Function, IntoLua, Lua, MetaMethod, MultiValue, Result, Table, UserData,
    UserDataFields, UserDataMethods, Value,
};
use rbx_dom::{Ref, Variant};

use super::api::Request;
use super::env::env;
use crate::engine::signal::{self, LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;

const OUT_OF_BOUNDS: &str = "Attempt to access a document with an index out of bounds";

/// `None` is the Command Bar's document.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct LuaScriptDocument(Option<Ref>);

fn document(lua: &Lua, script: Option<Ref>) -> Result<Value> {
    let key = format!("rbxn.script_document.{script:?}");
    if let Some(found) = lua.named_registry_value::<Option<Value>>(&key)? {
        return Ok(found);
    }
    let made = Value::UserData(lua.create_userdata(LuaScriptDocument(script))?);
    lua.set_named_registry_value(&key, made.clone())?;
    Ok(made)
}

fn source(lua: &Lua, script: Option<Ref>) -> String {
    let Some(script) = script else {
        return String::new();
    };
    match env(lua)
        .ctx
        .dom()
        .get(script)
        .and_then(|i| i.properties().get("Source"))
    {
        Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

fn int(args: &MultiValue, n: usize) -> Option<i64> {
    args.get(n + 1)
        .and_then(|v| v.as_i64().or(v.as_f64().map(|f| f as i64)))
}

/// The byte offset of (line, character), both from 1; errors past the end.
fn offset(text: &str, line: i64, character: i64) -> Result<usize> {
    let all = lines(text);
    let row = all
        .get((line - 1).max(-1) as usize)
        .filter(|_| line >= 1)
        .ok_or_else(|| mlua::Error::runtime(OUT_OF_BOUNDS))?;
    if character < 1 || character as usize > row.chars().count() + 1 {
        return Err(mlua::Error::runtime(OUT_OF_BOUNDS));
    }
    let start: usize = all[..(line - 1) as usize].iter().map(|l| l.len() + 1).sum();
    let within: usize = row
        .chars()
        .take(character as usize - 1)
        .map(char::len_utf8)
        .sum();
    Ok(start + within)
}

/// Applies `edits`; `false` and why for the Command Bar, as in Studio.
fn edit(lua: &Lua, script: Option<Ref>, edits: &[(String, [i64; 4])]) -> Result<MultiValue> {
    let Some(script) = script else {
        return Ok(MultiValue::from_vec(vec![
            Value::Boolean(false),
            Value::String(lua.create_string("Cannot apply edits from Lua to the CommandBar")?),
        ]));
    };
    let mut text = source(lua, Some(script));
    let mut ranges = Vec::new();
    for (new, [l1, c1, l2, c2]) in edits {
        let (a, b) = (offset(&text, *l1, *c1)?, offset(&text, *l2, *c2)?);
        if b < a {
            return Err(mlua::Error::runtime(OUT_OF_BOUNDS));
        }
        ranges.push((a, b, new.clone()));
    }
    // Every range is in the text as it was: apply from the end.
    ranges.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (a, b, new) in ranges {
        text.replace_range(a..b, &new);
    }
    let ctx = env(lua).ctx.clone();
    let value = Value::String(lua.create_string(&text)?);
    crate::property::set(&ctx, script, "Source", &value)?;
    Ok(done())
}

/// What a document call that went through returns: `true, nil`.
fn done() -> MultiValue {
    MultiValue::from_vec(vec![Value::Boolean(true), Value::Nil])
}

impl UserData for LuaScriptDocument {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let script = this.0;
            let f = |f: Function| Ok(Value::Function(f));
            match key.as_str() {
                "Name" => Ok(Value::String(lua.create_string(match script {
                    Some(_) => "ScriptDocument",
                    None => "CommandBar",
                })?)),
                "ClassName" => Ok(Value::String(lua.create_string("ScriptDocument")?)),
                "Parent" => Ok(Value::Nil),
                "SelectionChanged" | "ViewportChanged" => {
                    let owner = format!("{script:?}.{key}");
                    let key = SignalKey::new(Owner::Plugin, &owner);
                    Ok(Value::UserData(lua.create_userdata(LuaSignal(key))?))
                }
                "IsA" => f(lua.create_function(|_, (_, class): (Value, String)| {
                    Ok(matches!(class.as_str(), "ScriptDocument" | "Instance"))
                })?),
                "GetScript" => f(lua.create_function(move |lua, _: MultiValue| match script {
                    Some(script) => LuaInstance::new(script, env(lua).ctx.clone()).into_lua(lua),
                    None => Ok(Value::Nil),
                })?),
                "IsCommandBar" => {
                    f(lua.create_function(move |_, _: MultiValue| Ok(script.is_none()))?)
                }
                "IsReadOnly" => f(lua.create_function(|_, _: MultiValue| Ok(false))?),
                "GetReadOnlyReasons" => {
                    f(lua.create_function(|lua, _: MultiValue| lua.create_table())?)
                }
                "AddReadOnlyReasonsAsync" | "RemoveReadOnlyReasonsAsync" => {
                    f(lua.create_function(|_, _: MultiValue| Ok(()))?)
                }
                "GetLineCount" => f(lua.create_function(move |lua, _: MultiValue| {
                    Ok(lines(&source(lua, script)).len())
                })?),
                "GetLine" => f(lua.create_function(move |lua, args: MultiValue| {
                    let text = source(lua, script);
                    let all = lines(&text);
                    let n = int(&args, 0).unwrap_or(1);
                    if n < 1 || n as usize > all.len() {
                        return Err(mlua::Error::runtime(OUT_OF_BOUNDS));
                    }
                    Ok(all[n as usize - 1].to_owned())
                })?),
                "GetText" => f(lua.create_function(move |lua, args: MultiValue| {
                    let text = source(lua, script);
                    let Some(l1) = int(&args, 0) else {
                        return Ok(text);
                    };
                    let all = lines(&text);
                    let a = offset(&text, l1, int(&args, 1).unwrap_or(1))?;
                    let l2 = int(&args, 2).unwrap_or(all.len() as i64);
                    let c2 = match int(&args, 3) {
                        Some(c) => c,
                        None => all
                            .get((l2 - 1).max(0) as usize)
                            .map_or(1, |l| l.chars().count() as i64 + 1),
                    };
                    let b = offset(&text, l2, c2)?;
                    Ok(text.get(a..b.max(a)).unwrap_or_default().to_owned())
                })?),
                "GetSelection" => f(lua.create_function(|_, _: MultiValue| Ok((1, 1, 1, 1)))?),
                "GetSelectionStart" | "GetSelectionEnd" => {
                    f(lua.create_function(|_, _: MultiValue| Ok((1, 1)))?)
                }
                "GetSelectedText" => f(lua.create_function(|_, _: MultiValue| Ok(""))?),
                "HasSelectedText" => f(lua.create_function(|_, _: MultiValue| Ok(false))?),
                "GetViewport" => f(lua.create_function(move |lua, _: MultiValue| {
                    Ok((1, lines(&source(lua, script)).len()))
                })?),
                "EditTextAsync" => f(lua.create_function(move |lua, args: MultiValue| {
                    let new = args
                        .get(1)
                        .and_then(|v| v.as_string().map(|s| s.to_string_lossy().to_string()))
                        .ok_or_else(|| mlua::Error::runtime("Argument 1 missing or nil"))?;
                    let range = [1, 2, 3, 4].map(|i| int(&args, i).unwrap_or(0));
                    edit(lua, script, &[(new, range)])
                })?),
                "MultiEditTextAsync" => f(lua.create_function(move |lua, args: MultiValue| {
                    let Some(Value::Table(list)) = args.get(1) else {
                        return Err(mlua::Error::runtime("Argument 1 missing or nil"));
                    };
                    let mut edits = Vec::new();
                    for item in list.clone().sequence_values::<Table>() {
                        let item = item?;
                        let range: Table = item.get("range")?;
                        let start: Table = range.get("start")?;
                        let end: Table = range.get("end")?;
                        edits.push((
                            item.get::<String>("text")?,
                            [
                                start.get::<i64>("line")?,
                                start.get::<i64>("character")?,
                                end.get::<i64>("line")?,
                                end.get::<i64>("character")?,
                            ],
                        ));
                    }
                    edit(lua, script, &edits)
                })?),
                "ReviewableTextEditsAsync" => {
                    f(lua.create_function(move |lua, _: MultiValue| {
                        Ok(MultiValue::from_vec(vec![
                            Value::Boolean(false),
                            Value::String(lua.create_string("Reviewable edits are not supported")?),
                        ]))
                    })?)
                }
                "ForceSetSelectionAsync" | "RequestSetSelectionAsync" => {
                    f(lua.create_function(move |lua, args: MultiValue| {
                        let text = source(lua, script);
                        let (line, character) =
                            (int(&args, 0).unwrap_or(0), int(&args, 1).unwrap_or(0));
                        offset(&text, line, character)?;
                        Ok(done())
                    })?)
                }
                "CloseAsync" => f(lua.create_function(move |lua, _: MultiValue| {
                    let script = script
                        .ok_or_else(|| mlua::Error::runtime("Cannot close the Command Bar"))?;
                    env(lua)
                        .shared
                        .borrow_mut()
                        .requests
                        .push(Request::CloseScript(script));
                    Ok(done())
                })?),
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of ScriptDocument \"ScriptDocument\""
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("ScriptDocument"));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(other
                .borrow::<LuaScriptDocument>()
                .is_ok_and(|o| *o == *this))
        });
    }
}

fn open_scripts(lua: &Lua) -> Vec<Ref> {
    env(lua).shared.borrow().context.scripts.clone()
}

fn script_arg(value: Option<&Value>) -> Option<Ref> {
    value
        .and_then(Value::as_userdata)
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
}

/// The registered autocomplete and analysis callbacks, by name.
fn callbacks(lua: &Lua, kind: &str) -> Result<Table> {
    let key = format!("rbxn.script_editor.{kind}");
    if let Some(found) = lua.named_registry_value::<Option<Table>>(&key)? {
        return Ok(found);
    }
    let made = lua.create_table()?;
    lua.set_named_registry_value(&key, made.clone())?;
    Ok(made)
}

/// The `ScriptEditorService` members that deal in documents and callbacks.
pub(crate) fn service_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "GetScriptDocuments" => lua.create_function(|lua, _: MultiValue| {
            let list = lua.create_table()?;
            list.push(document(lua, None)?)?;
            for script in open_scripts(lua) {
                list.push(document(lua, Some(script))?)?;
            }
            Ok(list)
        })?,
        "FindScriptDocument" => lua.create_function(|lua, args: MultiValue| {
            let script = script_arg(args.get(1))
                .ok_or_else(|| mlua::Error::runtime("Argument 1 missing or nil"))?;
            if open_scripts(lua).contains(&script) {
                document(lua, Some(script))
            } else {
                Ok(Value::Nil)
            }
        })?,
        "OpenScriptDocumentAsync" => lua.create_function(|lua, args: MultiValue| {
            let script = script_arg(args.get(1))
                .ok_or_else(|| mlua::Error::runtime("Argument 1 missing or nil"))?;
            env(lua)
                .shared
                .borrow_mut()
                .requests
                .push(Request::OpenScript { script, line: None });
            Ok(done())
        })?,
        "RegisterAutocompleteCallback" | "RegisterScriptAnalysisCallback" => {
            let kind = key.trim_start_matches("Register").to_owned();
            lua.create_function(
                move |lua, (_, name, _priority, callback): (Value, String, Value, Function)| {
                    let registered = callbacks(lua, &kind)?;
                    if registered.contains_key(name.as_str())? {
                        return Err(mlua::Error::runtime(format!(
                            "Already have callback with named {name}"
                        )));
                    }
                    registered.set(name, callback)
                },
            )?
        }
        "DeregisterAutocompleteCallback" | "DeregisterScriptAnalysisCallback" => {
            let kind = key.trim_start_matches("Deregister").to_owned();
            lua.create_function(move |lua, (_, name): (Value, String)| {
                let registered = callbacks(lua, &kind)?;
                if !registered.contains_key(name.as_str())? {
                    return Err(mlua::Error::runtime(format!(
                        "No callback with name {name}"
                    )));
                }
                registered.set(name, Value::Nil)
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// Fires `TextDocumentDidOpen`/`DidClose` for the tabs that opened or
/// closed since the editor's last report.
pub(crate) fn sync(lua: &Lua, before: &[Ref], now: &[Ref]) -> Result<()> {
    if before == now {
        return Ok(());
    }
    let Some(Value::UserData(service)) = super::world::service(lua, "ScriptEditorService")? else {
        return Ok(());
    };
    let service = service.borrow::<LuaInstance>()?.referent();
    for (event, list, other) in [
        ("TextDocumentDidClose", before, now),
        ("TextDocumentDidOpen", now, before),
    ] {
        for script in list.iter().filter(|s| !other.contains(s)) {
            let doc = document(lua, Some(*script))?;
            let key = SignalKey::new(Owner::Instance(service), event);
            signal::fire_values(lua, &key, MultiValue::from_vec(vec![doc]))?;
        }
    }
    Ok(())
}
