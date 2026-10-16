//! Debugger control for plugins — an rbx-native addition shaped after
//! Roblox's own `DebuggerManager`/`ScriptDebugger`, which real Studio only
//! exposes to the command bar.

use mlua::{
    IntoLua, Lua, MetaMethod, MultiValue, Result, Table, UserData, UserDataFields, UserDataMethods,
    Value,
};
use rbx_dom::Ref;

use super::api::{Arg, DebuggerRequest, DebuggerView, Permission, Request};
use super::env::env;
use super::native;
use super::signal::{fire, fire_values, LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;

const DENIED: &str = "this plugin lacks the 'debugger' permission";

struct LuaManager;

struct LuaScriptDebugger(Ref);

fn check(lua: &Lua) -> Result<()> {
    if native::allowed(lua, &Permission::Debugger) {
        Ok(())
    } else {
        Err(mlua::Error::runtime(DENIED))
    }
}

fn request(lua: &Lua, request: DebuggerRequest) -> Result<()> {
    check(lua)?;
    env(lua)
        .shared
        .borrow_mut()
        .requests
        .push(Request::Debugger(request));
    Ok(())
}

fn view(lua: &Lua) -> DebuggerView {
    env(lua).shared.borrow().context.debugger.clone()
}

fn referent(value: &Value) -> Result<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
        .ok_or_else(|| mlua::Error::runtime("invalid argument #1 (script expected)"))
}

fn instance_value(lua: &Lua, referent: Ref) -> Result<Value> {
    let ctx = env(lua).ctx.clone();
    LuaInstance::new(referent, ctx).into_lua(lua)
}

fn signal_value(lua: &Lua, owner: Owner, event: &str) -> Result<Value> {
    lua.create_userdata(LuaSignal(SignalKey::new(owner, event)))
        .map(Value::UserData)
}

fn breakpoint_table(lua: &Lua, script: Ref, line: u32, condition: Option<&str>) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("Line", line)?;
    table.set("Condition", condition)?;
    table.set("Script", instance_value(lua, script)?)?;
    Ok(table)
}

fn watch_table(lua: &Lua, expression: &str) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("Expression", expression)?;
    Ok(table)
}

fn expression(value: Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy().to_string()),
        Value::Table(t) => t.get("Expression"),
        _ => Err(mlua::Error::runtime(
            "invalid argument #1 (watch or expression expected)",
        )),
    }
}

fn debugger_value(lua: &Lua, script: Ref) -> Result<Value> {
    lua.create_userdata(LuaScriptDebugger(script))
        .map(Value::UserData)
}

impl UserData for LuaManager {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("AddDebugger", |lua, _, script: Value| {
            check(lua)?;
            debugger_value(lua, referent(&script)?)
        });
        methods.add_method("GetDebuggers", |lua, _, ()| {
            check(lua)?;
            let list = lua.create_table()?;
            let mut seen: Vec<Ref> = Vec::new();
            for (script, _, _) in &view(lua).breakpoints {
                if !seen.contains(script) {
                    seen.push(*script);
                    list.push(debugger_value(lua, *script)?)?;
                }
            }
            Ok(list)
        });
        methods.add_method("Resume", |lua, _, ()| request(lua, DebuggerRequest::Resume));
        methods.add_method("StepIn", |lua, _, ()| request(lua, DebuggerRequest::StepIn));
        methods.add_method("StepOver", |lua, _, ()| {
            request(lua, DebuggerRequest::StepOver)
        });
        methods.add_method("StepOut", |lua, _, ()| {
            request(lua, DebuggerRequest::StepOut)
        });
        methods.add_method("Stop", |lua, _, ()| request(lua, DebuggerRequest::Stop));
        methods.add_method("Run", |lua, _, script: Value| {
            request(
                lua,
                DebuggerRequest::Run {
                    script: referent(&script)?,
                },
            )
        });
        methods.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            match key.as_str() {
                "DebuggingEnabled" => Ok(Value::Boolean(true)),
                "DebuggerAdded" => signal_value(lua, Owner::Debugger, "DebuggerAdded"),
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of DebuggerManager"
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("DebuggerManager"));
    }
}

impl UserData for LuaScriptDebugger {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "SetBreakpoint",
            |lua, this, (line, condition): (u32, Option<String>)| {
                request(
                    lua,
                    DebuggerRequest::SetBreakpoint {
                        script: this.0,
                        line,
                        condition: condition.clone(),
                    },
                )?;
                // Listed at once, as in Studio; the editor's own view
                // replaces this one on the next dispatch.
                {
                    let env = env(lua);
                    let mut shared = env.shared.borrow_mut();
                    let breakpoints = &mut shared.context.debugger.breakpoints;
                    breakpoints.retain(|(script, at, _)| (*script, *at) != (this.0, line));
                    breakpoints.push((this.0, line, condition.clone()));
                }
                breakpoint_table(lua, this.0, line, condition.as_deref())
            },
        );
        methods.add_method("RemoveBreakpoint", |lua, this, line: u32| {
            request(
                lua,
                DebuggerRequest::RemoveBreakpoint {
                    script: this.0,
                    line,
                },
            )?;
            env(lua)
                .shared
                .borrow_mut()
                .context
                .debugger
                .breakpoints
                .retain(|(script, at, _)| (*script, *at) != (this.0, line));
            Ok(())
        });
        methods.add_method("GetBreakpoints", |lua, this, ()| {
            check(lua)?;
            let list = lua.create_table()?;
            for (script, line, condition) in &view(lua).breakpoints {
                if *script == this.0 {
                    list.push(breakpoint_table(lua, *script, *line, condition.as_deref())?)?;
                }
            }
            Ok(list)
        });
        methods.add_method("AddWatch", |lua, _, expr: String| {
            request(lua, DebuggerRequest::AddWatch(expr.clone()))?;
            watch_table(lua, &expr)
        });
        methods.add_method("RemoveWatch", |lua, _, watch: Value| {
            request(lua, DebuggerRequest::RemoveWatch(expression(watch)?))
        });
        methods.add_method("GetWatches", |lua, _, ()| {
            check(lua)?;
            let list = lua.create_table()?;
            for (expr, _) in &view(lua).watches {
                list.push(watch_table(lua, expr)?)?;
            }
            Ok(list)
        });
        methods.add_method("GetWatchValue", |lua, _, watch: Value| {
            check(lua)?;
            let expr = expression(watch)?;
            Ok(view(lua)
                .watches
                .into_iter()
                .find(|(e, _)| *e == expr)
                .and_then(|(_, value)| value))
        });
        methods.add_method("GetStack", |lua, this, ()| {
            check(lua)?;
            let list = lua.create_table()?;
            if let Some((script, _, stack)) = view(lua).paused {
                if script == this.0 {
                    for (function, line) in stack {
                        let frame = lua.create_table()?;
                        frame.set("Function", function)?;
                        frame.set("Line", line)?;
                        list.push(frame)?;
                    }
                }
            }
            Ok(list)
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let view = view(lua);
            let paused_here = view
                .paused
                .as_ref()
                .filter(|(script, _, _)| *script == this.0);
            match key.as_str() {
                "Script" => instance_value(lua, this.0),
                "CurrentLine" => Ok(Value::Integer(
                    paused_here.map_or(0, |(_, line, _)| i64::from(*line)),
                )),
                "IsDebugging" => Ok(Value::Boolean(view.running)),
                "IsPaused" => Ok(Value::Boolean(paused_here.is_some())),
                "EncounteredBreak" | "Resuming" | "BreakpointAdded" | "BreakpointRemoved"
                | "WatchAdded" | "WatchRemoved" => signal_value(lua, Owner::Instance(this.0), &key),
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of ScriptDebugger"
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("ScriptDebugger"));
    }
}

/// `DebuggerManager()` (global) and `plugin:GetDebuggerManager()`.
/// Manager: `DebuggingEnabled` (true), `AddDebugger(script)` → script
/// debugger, `GetDebuggers()` (one per script with breakpoints),
/// `Resume()`, `StepIn()`, `StepOver()`, `StepOut()`, `Stop()`,
/// `Run(script)` (rbx-native), `DebuggerAdded` signal.
/// Script debugger: `Script`, `CurrentLine`, `IsDebugging`, `IsPaused`,
/// `SetBreakpoint(line, condition?)` → `{Line, Condition, Script}`,
/// `RemoveBreakpoint(line)`, `GetBreakpoints()`, `AddWatch(expr)` →
/// `{Expression}`, `RemoveWatch(expr)`, `GetWatches()`,
/// `GetWatchValue(watchOrExpr)`, `GetStack()` → `{{Function, Line}}`;
/// signals (`Owner::Instance(script)` keys) `EncounteredBreak(line)`,
/// `Resuming`, `BreakpointAdded(bp)`, `BreakpointRemoved(bp)`,
/// `WatchAdded(w)`, `WatchRemoved(w)`. Every action is a
/// `Request::Debugger`; reads come from `Context::debugger`. A native
/// plugin without `Permission::Debugger` gets an error.
pub(crate) fn manager(lua: &Lua) -> Result<Value> {
    check(lua)?;
    lua.create_userdata(LuaManager).map(Value::UserData)
}

/// Installs the `DebuggerManager` global. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    lua.globals().set(
        "DebuggerManager",
        lua.create_function(|lua, ()| manager(lua))?,
    )
}

fn scripts_of(view: &DebuggerView) -> Vec<Ref> {
    let mut scripts: Vec<Ref> = view.breakpoints.iter().map(|(s, _, _)| *s).collect();
    if let Some((script, _, _)) = &view.paused {
        scripts.push(*script);
    }
    scripts
}

/// Fires the signals for what changed between two views (pause →
/// `EncounteredBreak`, unpause → `Resuming`, breakpoints/watches added or
/// removed).
pub(crate) fn changed(lua: &Lua, before: &DebuggerView, after: &DebuggerView) -> Result<()> {
    let key = |script: Ref, event: &str| SignalKey::new(Owner::Instance(script), event);
    let pause_of = |v: &DebuggerView| v.paused.as_ref().map(|(s, l, _)| (*s, *l));

    if pause_of(before) != pause_of(after) {
        if let Some((script, _)) = pause_of(before) {
            fire(lua, &key(script, "Resuming"), &[])?;
        }
        if let Some((script, line)) = pause_of(after) {
            fire(
                lua,
                &key(script, "EncounteredBreak"),
                &[Arg::Number(f64::from(line))],
            )?;
        }
    }

    let bp_arg = |script: Ref, line: u32, condition: &Option<String>| {
        Arg::Table(vec![
            ("Line".to_string(), Arg::Number(f64::from(line))),
            (
                "Condition".to_string(),
                condition.clone().map_or(Arg::Nil, Arg::String),
            ),
            ("Script".to_string(), Arg::Instance(script)),
        ])
    };
    for (script, line, condition) in &after.breakpoints {
        if !before
            .breakpoints
            .iter()
            .any(|(s, l, _)| s == script && l == line)
        {
            if !before.breakpoints.iter().any(|(s, _, _)| s == script) {
                let value = debugger_value(lua, *script)?;
                fire_values(
                    lua,
                    &SignalKey::new(Owner::Debugger, "DebuggerAdded"),
                    MultiValue::from_vec(vec![value]),
                )?;
            }
            fire(
                lua,
                &key(*script, "BreakpointAdded"),
                &[bp_arg(*script, *line, condition)],
            )?;
        }
    }
    for (script, line, condition) in &before.breakpoints {
        if !after
            .breakpoints
            .iter()
            .any(|(s, l, _)| s == script && l == line)
        {
            fire(
                lua,
                &key(*script, "BreakpointRemoved"),
                &[bp_arg(*script, *line, condition)],
            )?;
        }
    }

    let mut scripts = scripts_of(before);
    scripts.extend(scripts_of(after));
    scripts.dedup();
    let watch_arg = |expr: &str| {
        Arg::Table(vec![(
            "Expression".to_string(),
            Arg::String(expr.to_string()),
        )])
    };
    let fire_watch = |event: &str, expr: &str| -> Result<()> {
        fire(
            lua,
            &SignalKey::new(Owner::Debugger, event),
            &[watch_arg(expr)],
        )?;
        for script in &scripts {
            fire(lua, &key(*script, event), &[watch_arg(expr)])?;
        }
        Ok(())
    };
    for (expr, _) in &after.watches {
        if !before.watches.iter().any(|(e, _)| e == expr) {
            fire_watch("WatchAdded", expr)?;
        }
    }
    for (expr, _) in &before.watches {
        if !after.watches.iter().any(|(e, _)| e == expr) {
            fire_watch("WatchRemoved", expr)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::scheduler;
    use crate::plugin::testing::test_vm;

    fn setup() -> (Lua, std::rc::Rc<crate::plugin::env::Env>, Ref) {
        let mut dom = WeakDom::new();
        let script = dom.new_instance("Script", "S", None);
        let (lua, env, _) = test_vm(dom, false);
        install(&lua).unwrap();
        lua.globals()
            .set("s", LuaInstance::new(script, env.ctx.clone()))
            .unwrap();
        (lua, env, script)
    }

    fn run(lua: &Lua, src: &str) {
        scheduler::spawn(
            lua,
            lua.load(src).into_function().unwrap(),
            MultiValue::new(),
        )
        .unwrap();
    }

    #[test]
    fn set_breakpoint_pushes_request() {
        let (lua, env, script) = setup();
        run(
            &lua,
            "bp = DebuggerManager():AddDebugger(s):SetBreakpoint(5) line = bp.Line",
        );
        assert_eq!(lua.globals().get::<u32>("line").unwrap(), 5);
        assert!(env.shared.borrow().requests.contains(&Request::Debugger(
            DebuggerRequest::SetBreakpoint {
                script,
                line: 5,
                condition: None
            }
        )));
    }

    #[test]
    fn get_breakpoints_reads_view() {
        let (lua, env, script) = setup();
        env.shared.borrow_mut().context.debugger.breakpoints = vec![(script, 3, Some("x".into()))];
        run(
            &lua,
            "local b = DebuggerManager():AddDebugger(s):GetBreakpoints() n = #b line = b[1].Line cond = b[1].Condition",
        );
        assert_eq!(lua.globals().get::<u32>("n").unwrap(), 1);
        assert_eq!(lua.globals().get::<u32>("line").unwrap(), 3);
        assert_eq!(lua.globals().get::<String>("cond").unwrap(), "x");
    }

    #[test]
    fn pause_fires_encountered_break() {
        let (lua, _env, script) = setup();
        run(
            &lua,
            "DebuggerManager():AddDebugger(s).EncounteredBreak:Connect(function(l) hit = l end)",
        );
        let before = DebuggerView::default();
        let after = DebuggerView {
            paused: Some((script, 7, vec![])),
            running: true,
            ..Default::default()
        };
        changed(&lua, &before, &after).unwrap();
        assert_eq!(lua.globals().get::<u32>("hit").unwrap(), 7);
    }

    #[test]
    fn resume_and_step_over_requests() {
        let (lua, env, _) = setup();
        run(
            &lua,
            "DebuggerManager():Resume() DebuggerManager():StepOver()",
        );
        let shared = env.shared.borrow();
        assert!(shared
            .requests
            .contains(&Request::Debugger(DebuggerRequest::Resume)));
        assert!(shared
            .requests
            .contains(&Request::Debugger(DebuggerRequest::StepOver)));
    }
}
