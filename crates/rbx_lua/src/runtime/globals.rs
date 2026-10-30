//! The globals every VM starts from.

use mlua::{Lua, Value, Variadic};

use crate::ctx::Ctx;
use crate::game::LuaGame;
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnums;
use crate::output::LineKind;
use crate::printed;
use crate::{datatypes, instance, LuaError};

/// The globals both VMs start from: `print`, `warn`, `version`, the
/// datatypes, `Instance`, `Enum`, `game`, `workspace`, `utf8.graphemes`,
/// and `pcall`/`xpcall`/`coroutine.resume` that hand back binding errors
/// as strings.
///
/// Luau's own `loadstring`, `getfenv` and `setfenv` are left in place on
/// purpose: Studio runs the command bar (and plugins) at plugin security,
/// where `loadstring` works whatever `ServerScriptService.LoadStringEnabled`
/// says, and `getfenv`/`setfenv` still exist in Roblox's Luau, deprecated
/// rather than removed, so code pasted from Studio expects them.
pub(crate) fn install_globals(lua: &Lua, ctx: &Ctx) -> Result<(), LuaError> {
    let globals = lua.globals();

    globals.set("print", output_function(lua, ctx, LineKind::Print)?)?;
    globals.set("warn", output_function(lua, ctx, LineKind::Warn)?)?;

    globals.set(
        "version",
        lua.create_function(|_, ()| Ok(env!("CARGO_PKG_VERSION")))?,
    )?;

    datatypes::register(lua)?;
    globals.set("Instance", instance::constructors(lua, ctx.clone())?)?;
    globals.set("Enum", LuaEnums::new(ctx.clone()))?;
    globals.set("game", LuaGame::new(ctx.clone()))?;

    let workspace = {
        let dom = ctx.dom();
        dom.root_refs()
            .iter()
            .copied()
            .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == "Workspace"))
    };
    if let Some(referent) = workspace {
        globals.set("workspace", LuaInstance::new(referent, ctx.clone()))?;
        globals.set("Workspace", LuaInstance::new(referent, ctx.clone()))?;
    }

    install_graphemes(lua)?;
    install_error_strings(lua)
}

/// `utf8.graphemes(s, i?, j?)`: iterates the grapheme clusters of `s`
/// (bytes `i` to `j`), giving each one's first and last byte position.
fn install_graphemes(lua: &Lua) -> Result<(), LuaError> {
    use unicode_segmentation::UnicodeSegmentation;
    let ranges = lua.create_function(
        |lua, (text, from, to): (mlua::LuaString, Option<i64>, Option<i64>)| {
            let bytes = text.as_bytes();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let len = text.len() as i64;
            let from = from.unwrap_or(1).max(1);
            let to = to.unwrap_or(len).min(len);
            let out = lua.create_table()?;
            for (start, cluster) in text.grapheme_indices(true) {
                let first = start as i64 + 1;
                let last = start as i64 + cluster.len() as i64;
                if first >= from && last <= to {
                    out.push(lua.create_sequence_from([first, last])?)?;
                }
            }
            Ok(out)
        },
    )?;
    let graphemes: mlua::Function = lua
        .load(
            r#"
            local ranges = ...
            return function(s, i, j)
                local list, k = ranges(s, i, j), 0
                return function()
                    k += 1
                    local r = list[k]
                    if r then return r[1], r[2] end
                end
            end
            "#,
        )
        .set_name("=utf8")
        .call(ranges)?;
    lua.globals()
        .get::<mlua::Table>("utf8")?
        .set("graphemes", graphemes)
}

/// What a script sees of an error a binding raised: Roblox hands scripts
/// the bare message ("Part is not a valid member of Workspace"), never an
/// error object, and code such as Roact formats it with `%s`.
fn error_text(error: &LuaError) -> String {
    match error {
        LuaError::CallbackError { cause, .. } => error_text(cause),
        LuaError::RuntimeError(message) => message.clone(),
        LuaError::SyntaxError { message, .. } => message.clone(),
        LuaError::WithContext { cause, .. } => error_text(cause),
        other => other.to_string(),
    }
}

/// Wraps `pcall`, `xpcall` and `coroutine.resume` so a caught binding error
/// comes back as its message string (see `error_text`); errors scripts
/// raise themselves (strings, tables) pass through untouched.
fn install_error_strings(lua: &Lua) -> Result<(), LuaError> {
    let text = lua.create_function(|lua, value: Value| match value {
        Value::Error(error) => Ok(Value::String(lua.create_string(error_text(&error))?)),
        other => Ok(other),
    })?;
    lua.load(
        r#"
        local text, rawpcall, rawxpcall, resume = ...
        local function caught(ok, ...)
            if ok then
                return true, ...
            end
            return false, text((...))
        end
        pcall = function(f, ...)
            return caught(rawpcall(f, ...))
        end
        xpcall = function(f, handler, ...)
            return rawxpcall(f, function(e)
                return handler(text(e))
            end, ...)
        end
        coroutine.resume = function(co, ...)
            return caught(resume(co, ...))
        end
        "#,
    )
    .set_name("=errors")
    .call::<()>((
        text,
        lua.globals().get::<Value>("pcall")?,
        lua.globals().get::<Value>("xpcall")?,
        lua.globals()
            .get::<mlua::Table>("coroutine")?
            .get::<Value>("resume")?,
    ))
}

/// `print` or `warn`: each argument through `printed::part`, so tables keep
/// their tree for the Output's expandable rows.
/// Roblox's `printidentity` for code that runs at security `identity`
/// (the command bar's 4, MCP's 10): prints `Current identity is 4`, or
/// `<prefix> 4` when given one.
pub(crate) fn printidentity(lua: &Lua, ctx: &Ctx, identity: u8) -> mlua::Result<mlua::Function> {
    let ctx = ctx.clone();
    lua.create_function(move |_, prefix: Option<String>| {
        let prefix = prefix.unwrap_or_else(|| "Current identity is".to_owned());
        ctx.print(
            LineKind::Print,
            vec![printed::PrintPart::Text(format!("{prefix} {identity}"))],
        );
        Ok(())
    })
}

fn output_function(lua: &Lua, ctx: &Ctx, kind: LineKind) -> Result<mlua::Function, LuaError> {
    let ctx = ctx.clone();
    lua.create_function(move |_, args: Variadic<Value>| {
        let mut parts = Vec::with_capacity(args.len());
        for value in args.iter() {
            parts.push(printed::part(value)?);
        }
        ctx.print(kind, parts);
        Ok(())
    })
}
