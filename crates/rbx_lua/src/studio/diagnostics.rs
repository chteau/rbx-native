//! `TestService` and `Stats`, open to game scripts as much as to plugins
//! and the Command Bar. The output lines and counts were read from real
//! Studio 0.748 (2026-11-21): `Message` and `Checkpoint` are Info lines,
//! `Error`, `Fail`, `Require(false)` and `Check(false)` errors (the
//! last two "fatal" when they stop the test), `Warn(false)` a warning;
//! every check, error and failure counts in `TestCount`.

use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::engine;
use crate::instance::LuaInstance;
use crate::output::LineKind;

fn arg(args: &MultiValue, n: usize) -> Value {
    args.get(n + 1).cloned().unwrap_or(Value::Nil)
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.to_string_lossy().to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// "TestService.Workspace(9): text", as Studio heads a line with where
/// the test said it came from.
fn located(ctx: &Ctx, source: &Value, line: &Value, text: &str) -> String {
    let source = source
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .and_then(|i| crate::instance::tree::full_name(&ctx.dom(), i.referent()));
    let line = line.as_i64().or(line.as_f64().map(|n| n as i64));
    match (source, line) {
        (Some(source), Some(line)) => format!("TestService.{source}({line}): {text}"),
        (Some(source), None) => format!("TestService.{source}(0): {text}"),
        (None, _) => format!("TestService: {text}"),
    }
}

fn bump(ctx: &Ctx, referent: Ref, property: &str) {
    let mut dom = ctx.dom_mut();
    if let Some(instance) = dom.get_mut(referent) {
        let properties = instance.properties_mut();
        let now = match properties.get(property) {
            Some(Variant::Int32(n)) => *n,
            _ => 0,
        };
        properties.insert(property.into(), Variant::Int32(now + 1));
    }
}

fn say(lua: &Lua, kind: LineKind, text: String) {
    engine(lua).host.output(kind, text);
}

pub(crate) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    match class {
        "TestService" => test_service(lua, ctx, referent, key),
        "Stats" => stats(lua, key),
        _ => Ok(None),
    }
}

fn test_service(lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match key {
        "Message" | "Checkpoint" => {
            let checkpoint = key == "Checkpoint";
            lua.create_function(move |lua, args: MultiValue| {
                let mut said = text(&arg(&args, 0));
                if checkpoint {
                    said = format!("checkpoint {said}");
                }
                say(lua, LineKind::Print, located(&ctx, &arg(&args, 1), &arg(&args, 2), &said));
                Ok(())
            })?
        }
        "Error" | "Fail" => {
            let fatal = key == "Fail";
            lua.create_function(move |lua, args: MultiValue| {
                let said = text(&arg(&args, 0));
                let said = if fatal { format!("fatal {said}") } else { said };
                bump(&ctx, referent, "ErrorCount");
                bump(&ctx, referent, "TestCount");
                say(lua, LineKind::Error, located(&ctx, &arg(&args, 1), &arg(&args, 2), &said));
                Ok(())
            })?
        }
        "Check" | "Require" | "Warn" => {
            let which = key.to_owned();
            lua.create_function(move |lua, args: MultiValue| {
                bump(&ctx, referent, "TestCount");
                if truthy(&arg(&args, 0)) {
                    return Ok(());
                }
                let said = text(&arg(&args, 1));
                let (kind, said, count) = match which.as_str() {
                    "Warn" => (LineKind::Warn, said, "WarnCount"),
                    "Require" => (LineKind::Error, format!("fatal {said}"), "ErrorCount"),
                    _ => (LineKind::Error, said, "ErrorCount"),
                };
                bump(&ctx, referent, count);
                say(lua, kind, located(&ctx, &arg(&args, 2), &arg(&args, 3), &said));
                Ok(())
            })?
        }
        "Done" | "StartTestSession" | "StopTestSession" | "ResetTestRegistry"
        | "ResetTestControl" | "SetTestControl" | "SignalProfilingCapture"
        | "SignalProfilingStart" | "SignalProfilingStop" | "TakeSnapshot" => {
            lua.create_function(|_, _: MultiValue| Ok(()))?
        }
        "ListTests" => lua.create_function(|_, _: MultiValue| -> Result<()> {
            Err(mlua::Error::runtime(
                "TestService:ListTests only works while playtesting (press Play/Run in Studio) or on a Team Test server -- not in Edit mode or the command bar. (TestService.Enabled is false here.)",
            ))
        })?,
        "GetTestControls" | "GetTestControlSchema" | "ScopeTime"
        | "getTestSessionProviderStats" => lua.create_function(|lua, _: MultiValue| lua.create_table())?,
        "isFeatureEnabled" => lua.create_function(|_, _: MultiValue| Ok(false))?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// What the process holds, in MB: the resident set, as Studio's figure is
/// its own process's.
fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

/// Studio answers `GetMemoryUsageMbForTag` with 0 for every tag outside a
/// playtest; the rest of `Stats`' functions need a capability no script has.
fn stats(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "GetTotalMemoryUsageMb" => lua.create_function(|_, _: MultiValue| Ok(resident_mb()))?,
        "GetMemoryUsageMbForTag" => lua.create_function(|_, args: MultiValue| {
            let tag = arg(&args, 0);
            if tag
                .as_userdata()
                .is_none_or(|u| !u.is::<crate::lua_enum::LuaEnumItem>())
            {
                return Err(mlua::Error::runtime("Argument 1 missing or nil"));
            }
            Ok(0.0)
        })?,
        "GetMemoryCategoryNames"
        | "GetMemoryUsageMbAllCategories"
        | "GetMemoryUsageMbAllCategoriesFiltered"
        | "GetMemoryUsageMbFiltered"
        | "GetHarmonyQualityLevel"
        | "SetHarmonyMemoryTarget"
        | "ResetHarmonyMemoryTarget" => {
            let message =
                format!("The current thread cannot call '{key}' (lacking capability InternalTest)");
            lua.create_function(move |_, _: MultiValue| -> Result<()> {
                Err(mlua::Error::runtime(message.clone()))
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
