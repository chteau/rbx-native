//! Building one plugin's VM and running its scripts.

use std::time::Instant;

use std::rc::Rc;

use mlua::chunk::Compiler;
use mlua::{Lua, MultiValue, Result, Table, Value, Variadic, VmState};
use rbx_dom::{Ref, Variant};

use super::api::OutputKind;
use super::env::{env, Env};
use super::events::PluginHooks;
use super::{debugger_api, native, plugin_object, scheduler, services};
use crate::hooks::HookSlot;
use crate::instance::tree::full_name;
use crate::instance::LuaInstance;
use crate::printed;

const TIMEOUT: &str = "Script timeout: exhausted allowed execution time";

/// A sandboxed Luau VM for one plugin: the command bar's globals
/// (`runtime::install_globals` — `print` there is replaced so output goes
/// to `Shared::output` as the plugin's `Print` lines), plus `warn`
/// (`Warn`), `require`, `typeof`-compatible datatypes,
/// `scheduler::install`, `plugin_object::install`,
/// `services::install_settings`, `debugger_api::install`,
/// `native::install`; `Env` as app data and `crate::hooks::HookSlot(
/// PluginHooks)`; compiled with `game`/`workspace`/`Workspace`/`script`
/// mutable; an interrupt that errors with "Script timeout: exhausted
/// allowed execution time" once `Shared::deadline` has passed. Sandboxed
/// last.
pub(crate) fn build(env: Rc<Env>) -> Result<Lua> {
    let lua = Lua::new();
    lua.set_compiler(Compiler::new().set_mutable_globals([
        "game",
        "workspace",
        "Workspace",
        "script",
    ]));
    lua.set_app_data(env.clone());
    lua.set_app_data(HookSlot(Rc::new(PluginHooks)));

    crate::runtime::install_globals(&lua, &env.ctx)?;
    let globals = lua.globals();
    globals.set("print", output_function(&lua, OutputKind::Print)?)?;
    globals.set("warn", output_function(&lua, OutputKind::Warn)?)?;
    globals.set("require", super::require::require_for(&lua, Value::Nil)?)?;
    // Deprecated spellings Roblox still answers to.
    globals.set("Game", globals.get::<Value>("game")?)?;

    scheduler::install(&lua)?;
    plugin_object::install(&lua)?;
    services::install_settings(&lua)?;
    debugger_api::install(&lua)?;
    native::install(&lua)?;

    let shared = env.shared.clone();
    lua.set_interrupt(move |_| {
        let expired = shared
            .try_borrow()
            .ok()
            .and_then(|shared| shared.deadline)
            .is_some_and(|deadline| Instant::now() >= deadline);
        if expired {
            Err(mlua::Error::runtime(TIMEOUT))
        } else {
            Ok(VmState::Continue)
        }
    });

    lua.sandbox(true)?;
    Ok(lua)
}

fn output_function(lua: &Lua, kind: OutputKind) -> Result<mlua::Function> {
    lua.create_function(move |lua, args: Variadic<Value>| {
        let mut parts = Vec::with_capacity(args.len());
        for value in args.iter() {
            parts.push(printed::part(value)?);
        }
        let env = env(lua);
        env.shared
            .borrow_mut()
            .push_output(&env.name, kind, printed::text(&parts));
        Ok(())
    })
}

/// The chunk for `script`, ready to call, in its own environment.
/// Compiles a script's `Source` with its own environment: `script`, a
/// `require` bound to it, and the plugin's shared `_G`/`shared` tables,
/// falling through to the globals. `None` (after reporting it) when the
/// source does not compile.
pub(crate) fn compile(lua: &Lua, script: Ref) -> Result<Option<mlua::Function>> {
    let env = env(lua);
    let (source, name) = {
        let dom = env.ctx.dom();
        let Some(instance) = dom.get(script) else {
            return Ok(None);
        };
        let source = match instance.properties().get("Source") {
            Some(Variant::String(text)) => text.clone(),
            _ => String::new(),
        };
        let name = full_name(&dom, script).unwrap_or_else(|| instance.name().to_string());
        (source, name)
    };

    let scope = lua.create_table()?;
    let this = mlua::IntoLua::into_lua(LuaInstance::new(script, env.ctx.clone()), lua)?;
    scope.set("script", this.clone())?;
    scope.set("require", super::require::require_for(lua, this)?)?;
    let (g, shared) = super::require::script_globals(lua)?;
    scope.set("_G", g)?;
    scope.set("shared", shared)?;
    scope.set("getfenv", super::require::getfenv_for(lua, &scope)?)?;
    let meta: Table = lua.create_table()?;
    meta.set("__index", lua.globals())?;
    scope.set_metatable(Some(meta))?;

    match lua
        .load(source)
        .set_name(format!("={name}"))
        .set_environment(scope)
        .into_function()
    {
        Ok(function) => Ok(Some(function)),
        Err(error) => {
            env.shared
                .borrow_mut()
                .push_output(&env.name, OutputKind::Error, error.to_string());
            Ok(None)
        }
    }
}

/// Runs a `Script`'s `Source` in a new thread (`scheduler::spawn`), as a
/// chunk named after the script's full name, in an environment whose
/// `script` is that instance (other names fall through to the globals).
/// A compile error is reported to the Output like a runtime one.
pub(crate) fn run_script(lua: &Lua, script: Ref) -> Result<()> {
    match compile(lua, script) {
        Ok(Some(function)) => scheduler::spawn(lua, function, MultiValue::new()),
        Ok(None) => Ok(()),
        // Binding the script's `require` runs Lua, so a spent budget can
        // stop it before the script starts: report it like a runtime error.
        Err(error) => {
            let env = env(lua);
            env.shared
                .borrow_mut()
                .push_output(&env.name, OutputKind::Error, error.to_string());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::testing::test_vm;

    fn script(dom: &mut WeakDom, class: &str, name: &str, parent: Ref, source: &str) -> Ref {
        let r = dom.new_instance(class, name, Some(parent));
        dom.get_mut(r)
            .unwrap()
            .properties_mut()
            .insert("Source".into(), Variant::String(source.to_string()));
        r
    }

    fn vm_with(setup: impl FnOnce(&mut WeakDom, Ref) -> Ref) -> (Lua, Rc<Env>, Ref) {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let main = setup(&mut dom, ws);
        let (_, env, _) = test_vm(dom, false);
        let lua = build(env.clone()).expect("build");
        (lua, env, main)
    }

    fn lines(env: &Env) -> Vec<(OutputKind, String)> {
        env.shared
            .borrow()
            .output
            .iter()
            .map(|l| (l.kind, l.text.clone()))
            .collect()
    }

    #[test]
    fn print_warn_and_error_reach_output() {
        let (lua, env, main) =
            vm_with(|dom, ws| script(dom, "Script", "S", ws, "print('a', 1) warn('b') error('c')"));
        run_script(&lua, main).unwrap();
        let out = lines(&env);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], (OutputKind::Print, "a 1".to_string()));
        assert_eq!(out[1], (OutputKind::Warn, "b".to_string()));
        assert_eq!(out[2].0, OutputKind::Error);
        assert!(out[2].1.contains('c'));
    }

    #[test]
    fn require_runs_module_once() {
        let (lua, env, main) = vm_with(|dom, ws| {
            script(dom, "ModuleScript", "M", ws, "print('load') return {n = 7}");
            script(
                dom,
                "Script",
                "S",
                ws,
                "local a = require(script.Parent.M) local b = require(script.Parent.M) \
                 print(a.n, a == b)",
            )
        });
        run_script(&lua, main).unwrap();
        let out: Vec<String> = lines(&env).into_iter().map(|l| l.1).collect();
        assert_eq!(out, vec!["load".to_string(), "7 true".to_string()]);
    }

    #[test]
    fn script_parent_resolves() {
        let (lua, env, main) =
            vm_with(|dom, ws| script(dom, "Script", "S", ws, "print(script.Parent.Name)"));
        run_script(&lua, main).unwrap();
        assert_eq!(
            lines(&env),
            vec![(OutputKind::Print, "Workspace".to_string())]
        );
    }

    #[test]
    fn infinite_loop_times_out() {
        let (lua, env, main) =
            vm_with(|dom, ws| script(dom, "Script", "S", ws, "while true do end"));
        env.shared.borrow_mut().deadline = Instant::now().checked_sub(Duration::from_secs(1));
        run_script(&lua, main).unwrap();
        let out = lines(&env);
        assert!(out
            .iter()
            .any(|(kind, text)| *kind == OutputKind::Error && text.contains(TIMEOUT)));
    }
}
