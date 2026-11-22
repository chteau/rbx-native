//! Studio-only members of services and instances that need no editor
//! behind them, for plugins and the Command Bar. What each returns or
//! raises was read from real Studio 0.748's Command Bar in edit mode
//! (2026-11-21); the editor-facing ones (`ZoomToExtents`, script documents,
//! file prompts) are the plugin host's.

use mlua::{Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::signal::{LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;

fn func(
    lua: &Lua,
    f: impl Fn(&Lua, MultiValue) -> Result<Value> + 'static,
) -> Result<Option<Value>> {
    Ok(Some(Value::Function(lua.create_function(f)?)))
}

fn void(lua: &Lua, f: impl Fn(&Lua, MultiValue) -> Result<()> + 'static) -> Result<Option<Value>> {
    Ok(Some(Value::Function(lua.create_function(f)?)))
}

fn fail(message: &str) -> mlua::Error {
    mlua::Error::runtime(message.to_owned())
}

fn signal(lua: &Lua, referent: Ref, event: &str) -> Result<Option<Value>> {
    let key = SignalKey::new(Owner::Instance(referent), event);
    Ok(Some(Value::UserData(lua.create_userdata(LuaSignal(key))?)))
}

pub(crate) fn enum_item(lua: &Lua, ctx: &Ctx, enum_name: &str, item: &str) -> Result<Value> {
    let value = ctx.database().enum_value(enum_name, item).unwrap_or(0);
    LuaEnumItem::new(enum_name.to_owned(), item.to_owned(), value, ctx.clone()).interned(lua)
}

/// The `n`th argument after `self`.
fn arg(args: &MultiValue, n: usize) -> Value {
    args.get(n + 1).cloned().unwrap_or(Value::Nil)
}

fn instance_arg(args: &MultiValue, n: usize) -> Option<Ref> {
    arg(args, n)
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
}

fn registry(lua: &Lua, key: &str) -> Result<Table> {
    if let Some(found) = lua.named_registry_value::<Option<Table>>(key)? {
        return Ok(found);
    }
    let made = lua.create_table()?;
    lua.set_named_registry_value(key, made.clone())?;
    Ok(made)
}

/// Members of `game` itself.
pub(crate) fn game_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    match key {
        "PlaceId" | "GameId" => {
            let ids = registry(lua, "rbxn.place_ids")?;
            Ok(match ids.get::<Value>(key)? {
                Value::Nil => None,
                id => Some(id),
            })
        }
        "SetPlaceId" | "SetUniverseId" => {
            let field = if key == "SetPlaceId" {
                "PlaceId"
            } else {
                "GameId"
            };
            void(lua, move |lua, args| {
                let id = arg(&args, 0)
                    .as_f64()
                    .or(arg(&args, 0).as_i64().map(|i| i as f64));
                let id = id.ok_or_else(|| fail("Argument 1 missing or nil"))?;
                registry(lua, "rbxn.place_ids")?.set(field, id as i64)?;
                let changed = crate::engine::events::game_signal(format!("Changed:{field}"));
                crate::engine::signal::fire(lua, &changed.0, &[])?;
                Ok(())
            })
        }
        // A header row of field names, then one row per job, as Studio
        // gives it; this runtime's jobs are the frame loop's.
        "GetJobsInfo" => func(lua, |lua, _| {
            let rows = lua.create_table()?;
            rows.push(lua.create_sequence_from([
                "name",
                "averageDutyCycle",
                "averageStepsPerSecond",
                "averageStepTime",
                "averageError",
                "isRunning",
            ])?)?;
            for job in ["Heartbeat", "Render", "Physics", "LuaGc", "Sound"] {
                let row = lua.create_table()?;
                row.push(job)?;
                for _ in 0..4 {
                    row.push(0.0)?;
                }
                row.push(false)?;
                rows.push(row)?;
            }
            Ok(Value::Table(rows))
        }),
        _ => Ok(None),
    }
}

/// Members of instances of `class`; `None` for anything else.
pub(crate) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    if let Some(value) = super::diagnostics::member(lua, ctx, referent, class, key)? {
        return Ok(Some(value));
    }
    let is = |base: &str| ctx.database().is_subclass_of(class, base);
    match (class, key) {
        ("ScriptContext", "SetTimeout") => void(lua, |lua, args| {
            let seconds = arg(&args, 0).as_f64().unwrap_or(0.0);
            lua.set_named_registry_value("rbxn.script_timeout", seconds)?;
            Ok(())
        }),
        ("Workspace", "ApplyRecommendedStreamingSettings") => {
            let ctx = ctx.clone();
            func(lua, move |_, _| {
                let mut dom = ctx.dom_mut();
                if let Some(instance) = dom.get_mut(referent) {
                    instance
                        .properties_mut()
                        .insert("StreamingEnabled".into(), Variant::Bool(true));
                }
                Ok(Value::Boolean(true))
            })
        }
        // Edit mode has no simulation: Studio leaves the parts where they are.
        (_, "StepPhysics") if is("WorldRoot") => void(lua, |_, _| Ok(())),
        // With nothing jointed to it, Studio puts the part at the target.
        (_, "IKMoveTo") if is("WorldRoot") => {
            let ctx = ctx.clone();
            void(lua, move |_, args| {
                let part =
                    instance_arg(&args, 0).ok_or_else(|| fail("Argument 1 missing or nil"))?;
                let target = arg(&args, 1);
                crate::property::set(&ctx, part, "CFrame", &target)?;
                Ok(())
            })
        }
        (_, "SetOutgoingKBPSLimit") if is("NetworkPeer") => void(lua, |_, _| Ok(())),
        ("Player", "SetAccountAge") => {
            let ctx = ctx.clone();
            void(lua, move |_, args| {
                crate::property::set(&ctx, referent, "AccountAge", &arg(&args, 0))?;
                Ok(())
            })
        }
        ("Player", "SetSuperSafeChat") => void(lua, |_, _| Ok(())),
        ("Players", "Chat") => func(lua, |_, _| Err(fail("No local Player to chat from"))),
        ("Players", "TeamChat") => void(lua, |_, _| Ok(())),
        ("Players", "SetChatStyle") => void(lua, |_, _| Ok(())),
        ("ScriptProfilerService", _) => profiler(lua, referent, key),
        ("HeapProfilerService", "ClientRequestDataAsync") => void(lua, |_, args| {
            player_arg(
                &args,
                "First argument to HeapProfilerClientRequestData must be a valid Player",
            )?;
            Ok(())
        }),
        ("HeapProfilerService", "ServerRequestDataAsync") => func(lua, |_, _| {
            Err(fail("Heap profiling is only available in a running game"))
        }),
        ("HeapProfilerService" | "InstanceFileSyncService", "OnNewData" | "StatusChanged")
        | ("PluginConnectionService", "Connected")
        | ("ScriptDebuggerService", "Resumed")
        | ("StudioDeviceSimulatorService", "ConfigurationChanged") => signal(lua, referent, key),
        ("StudioCaptureService", "CanCaptureScreenshot") => {
            func(lua, |_, _| Ok(Value::Boolean(false)))
        }
        ("StudioCaptureService", "RequestScreenshotPermissionAsync") => {
            func(lua, |_, _| Ok(Value::Boolean(false)))
        }
        ("StudioCaptureService", "CaptureScreenshot") => func(lua, |_, _| {
            Err(fail("Screenshot capture permission has not been granted"))
        }),
        ("StudioTestService", "EditModeActive") => Ok(Some(Value::Boolean(true))),
        ("InstanceFileSyncService", "GetAllInstances") => {
            func(lua, |lua, _| Ok(Value::Table(lua.create_table()?)))
        }
        ("InstanceFileSyncService", "GetStatus") => {
            let ctx = ctx.clone();
            func(lua, move |lua, _| {
                enum_item(lua, &ctx, "InstanceFileSyncStatus", "NotSynced")
            })
        }
        ("InstanceFileSyncService", "GetSyncedInstance") => func(lua, |_, _| Ok(Value::Nil)),
        ("PluginConnectionService", "CanHaveConnectionType") => func(lua, |_, args| {
            let item = arg(&args, 0);
            let name = item
                .as_userdata()
                .and_then(|u| {
                    u.borrow::<LuaEnumItem>()
                        .ok()
                        .map(|i| i.item_name().to_owned())
                })
                .unwrap_or_default();
            Ok(Value::Boolean(name == "Test"))
        }),
        ("PluginConnectionService", "GetPluginConnectionsOfType") => {
            func(lua, |lua, _| Ok(Value::Table(lua.create_table()?)))
        }
        ("ScriptDebuggerService", _) => script_debugger(lua, key),
        ("StudioDeviceSimulatorService", _) => super::devices::member(lua, ctx, referent, key),
        _ => Ok(None),
    }
}

fn player_arg(args: &MultiValue, message: &str) -> Result<()> {
    match arg(args, 0)
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
    {
        Some(_) => Ok(()),
        None => Err(fail(message)),
    }
}

/// `ScriptProfilerService`: in edit mode the server half starts and stops
/// with nothing to report, the client half wants a player.
fn profiler(lua: &Lua, referent: Ref, key: &str) -> Result<Option<Value>> {
    match key {
        "ServerStart" | "ServerStop" | "ServerRequestData" => void(lua, |_, _| Ok(())),
        "ClientStart" | "ClientStop" | "ClientRequestData" => {
            let message = format!("First argument to ScriptProfiler{key} must be a valid Player");
            void(lua, move |_, args| {
                player_arg(&args, &message)?;
                Ok(())
            })
        }
        // Studio checks the version field; a string that isn't JSON at all
        // gives nothing back.
        "DeserializeJSON" => {
            let f = lua.create_function(|lua, args: MultiValue| {
                let text = arg(&args, 0);
                let json = text.as_string().and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(&s.to_string_lossy()).ok()
                });
                let Some(json) = json else {
                    return Ok(MultiValue::new());
                };
                if json.get("Version").and_then(|v| v.as_u64()) != Some(2) {
                    return Err(fail("Unsupported ScriptProfiler JSON version"));
                }
                Ok(MultiValue::from_vec(vec![crate::engine::json::from_json(
                    lua, &json,
                )?]))
            })?;
            Ok(Some(Value::Function(f)))
        }
        "OnNewData" => signal(lua, referent, key),
        _ => Ok(None),
    }
}

/// `ScriptDebuggerService` with nothing paused: no threads, no frames, the
/// breakpoints kept per script.
fn script_debugger(lua: &Lua, key: &str) -> Result<Option<Value>> {
    match key {
        "GetThreads" | "GetRootVariables" | "GetVariables" => {
            func(lua, |lua, _| Ok(Value::Table(lua.create_table()?)))
        }
        "GetStackTrace" => func(lua, |_, _| Err(fail("Invalid threadId"))),
        "Pause" | "SetExceptionBreakMode" => void(lua, |_, _| Ok(())),
        "ClearBreakpoints" => void(lua, |lua, _| {
            lua.set_named_registry_value("rbxn.debugger_breakpoints", lua.create_table()?)?;
            Ok(())
        }),
        "Evaluate" => func(lua, |lua, _| {
            let next: i64 = lua
                .named_registry_value::<Option<i64>>("rbxn.debugger_refs")?
                .unwrap_or(2000)
                + 1;
            lua.set_named_registry_value("rbxn.debugger_refs", next)?;
            let result = lua.create_table()?;
            result.set("VariablesReference", next)?;
            result.set("Type", "Tuple")?;
            result.set("Result", "Tuple")?;
            Ok(Value::Table(result))
        }),
        "AddBreakpoint" => func(lua, |lua, args| {
            let script = instance_arg(&args, 0).ok_or_else(|| fail("Argument 1 missing or nil"))?;
            let spec = arg(&args, 1);
            let spec = spec
                .as_table()
                .ok_or_else(|| fail("Argument 2 missing or nil"))?;
            let line: Option<i64> = spec.get("Line")?;
            let line = line.ok_or_else(|| fail("required field 'Line' missing"))?;
            let breakpoint = lua.create_table()?;
            for pair in spec.pairs::<Value, Value>() {
                let (k, v) = pair?;
                breakpoint.set(k, v)?;
            }
            breakpoint.set("Line", line)?;
            breakpoint.set("Verified", true)?;
            let key = format!("{}:{line}", crate::engine::events::ref_key(script));
            registry(lua, "rbxn.debugger_breakpoints")?.set(key, breakpoint.clone())?;
            Ok(Value::Table(breakpoint))
        }),
        "RemoveBreakpoint" => func(lua, |lua, args| {
            let script = instance_arg(&args, 0).ok_or_else(|| fail("Argument 1 missing or nil"))?;
            let line = arg(&args, 1)
                .as_i64()
                .or(arg(&args, 1).as_f64().map(|f| f as i64))
                .unwrap_or(0);
            let key = format!("{}:{line}", crate::engine::events::ref_key(script));
            let table = registry(lua, "rbxn.debugger_breakpoints")?;
            let had = !matches!(table.get::<Value>(key.as_str())?, Value::Nil);
            table.set(key, Value::Nil)?;
            Ok(Value::Boolean(had))
        }),
        _ => Ok(None),
    }
}
