//! `RBXScriptSignal` / `RBXScriptConnection`.

use std::collections::HashMap;

use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, Result, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rbx_dom::Ref;

use super::{engine, scheduler};
use crate::plugin::api::{Arg, UiId};
use crate::plugin::arg;

const WAIT_KEY: &str = "__rbxn_signal_wait";

/// Who a signal belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    Instance(Ref),
    /// A toolbar button, plugin action, menu or widget.
    Ui(UiId),
    /// A service by class name (`"Selection"`, `"RunService"`, …).
    Service(&'static str),
    /// The `plugin` object itself (`Unloading`, `Deactivation`).
    Plugin,
    /// `DebuggerManager` (`DebuggerAdded`…) and script debuggers
    /// (`Instance(script)` is used for those).
    Debugger,
}

/// One signal: owner plus event name. Property signals are named
/// `"Changed:<Property>"`, attribute ones `"AttributeChanged:<Name>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SignalKey {
    pub(crate) owner: Owner,
    pub(crate) event: String,
}

impl SignalKey {
    pub(crate) fn new(owner: Owner, event: impl Into<String>) -> Self {
        SignalKey {
            owner,
            event: event.into(),
        }
    }
}

pub(crate) struct Connection {
    pub(crate) id: u32,
    pub(crate) func: Function,
    pub(crate) once: bool,
    /// The DOM's mutation count when it was made, until [`Signals::settle`]:
    /// events replayed from the change log (see [`fire_at`]) skip the
    /// changes logged before it, which Roblox would have fired before the
    /// connection existed.
    pub(crate) since: Option<u64>,
}

/// One VM's connections.
#[derive(Default)]
pub(crate) struct Signals {
    pub(crate) connections: HashMap<SignalKey, Vec<Connection>>,
    pub(crate) next: u32,
}

impl Signals {
    pub(crate) fn has(&self, key: &SignalKey) -> bool {
        self.connections.get(key).is_some_and(|c| !c.is_empty())
    }

    /// Whether any connection is on an instance in `refs`' owners
    /// (any event).
    pub(crate) fn watches_instance(&self, referent: Ref) -> bool {
        self.connections
            .iter()
            .any(|(key, c)| key.owner == Owner::Instance(referent) && !c.is_empty())
    }

    pub(crate) fn connect(
        &mut self,
        key: &SignalKey,
        func: Function,
        once: bool,
        since: Option<u64>,
    ) -> u32 {
        self.next += 1;
        let id = self.next;
        self.connections
            .entry(key.clone())
            .or_default()
            .push(Connection {
                id,
                func,
                once,
                since,
            });
        id
    }

    /// Once the change log a run made has been fired, every connection
    /// hears whatever comes next.
    pub(crate) fn settle(&mut self) {
        for connection in self.connections.values_mut().flatten() {
            connection.since = None;
        }
    }

    fn disconnect(&mut self, key: &SignalKey, id: u32) {
        if let Some(list) = self.connections.get_mut(key) {
            list.retain(|c| c.id != id);
            if list.is_empty() {
                self.connections.remove(key);
            }
        }
    }
}

/// The userdata scripts see. Methods: `Connect(fn)`, `Once(fn)`,
/// `ConnectParallel(fn)` (= `Connect`); `Wait()` is served through
/// `__index` as a yielding function (see `scheduler::yielding`) that
/// resumes with the next fire's arguments. `tostring` gives
/// `"Signal <event>"`.
#[derive(Clone)]
pub(crate) struct LuaSignal(pub(crate) SignalKey);

/// `Disconnect()`, `Connected` (bool field).
#[derive(Clone)]
pub(crate) struct LuaConnection {
    pub(crate) key: SignalKey,
    pub(crate) id: u32,
}

fn connect(lua: &Lua, key: &SignalKey, func: Function, once: bool) -> LuaConnection {
    let id = connect_now(lua, key, func, once);
    LuaConnection {
        key: key.clone(),
        id,
    }
}

fn wait_function(lua: &Lua) -> Result<Function> {
    if let Some(f) = lua.named_registry_value::<Option<Function>>(WAIT_KEY)? {
        return Ok(f);
    }
    let start = lua.create_function(|lua, this: AnyUserData| {
        let key = this.borrow::<LuaSignal>()?.0.clone();
        let id = scheduler::new_await(lua);
        let resolver = lua
            .create_function(move |lua, args: MultiValue| scheduler::resolve(lua, id, Ok(args)))?;
        connect_now(lua, &key, resolver, true);
        Ok(id)
    })?;
    let f = scheduler::yielding(lua, start)?;
    lua.set_named_registry_value(WAIT_KEY, f.clone())?;
    Ok(f)
}

impl UserData for LuaSignal {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RBXScriptSignal");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Connect", |lua, this, func: Function| {
            Ok(connect(lua, &this.0, func, false))
        });
        methods.add_method("ConnectParallel", |lua, this, func: Function| {
            Ok(connect(lua, &this.0, func, false))
        });
        methods.add_method("Once", |lua, this, func: Function| {
            Ok(connect(lua, &this.0, func, true))
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            match key.as_str() {
                "Wait" => wait_function(lua).map(Value::Function),
                "wait" => wait_function(lua).map(Value::Function),
                _ if key.starts_with(|c: char| c.is_ascii_lowercase()) => {
                    Ok(crate::compat::lowercase_method(lua, &key)?.unwrap_or(Value::Nil))
                }
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of Signal {}",
                    this.0.event
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Signal {}", this.0.event))
        });
    }
}

impl UserData for LuaConnection {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RBXScriptConnection");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Disconnect", |lua, this, ()| {
            engine(lua)
                .signals
                .borrow_mut()
                .disconnect(&this.key, this.id);
            Ok(())
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            match key.as_str() {
                "Connected" | "connected" => {
                    let engine = engine(lua);
                    let signals = engine.signals.borrow();
                    Ok(Value::Boolean(
                        signals
                            .connections
                            .get(&this.key)
                            .is_some_and(|c| c.iter().any(|c| c.id == this.id)),
                    ))
                }
                // `disconnect`, the deprecated spelling.
                _ => match crate::compat::lowercase_method(lua, &key)? {
                    Some(alias) => Ok(alias),
                    None => Err(mlua::Error::runtime(format!(
                        "{key} is not a valid member of RBXScriptConnection"
                    ))),
                },
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Connection"));
    }
}

/// Fires `key` in this VM: every connected function runs in its own thread
/// (`scheduler::spawn`) with `args` converted on the VM's `Engine::ctx`;
/// `Once` connections are removed before they run. A missing key is a
/// no-op.
pub(crate) fn fire(lua: &Lua, key: &SignalKey, args: &[Arg]) -> Result<()> {
    fire_args(lua, key, args, None)
}

/// [`fire`] for a change replayed from the DOM's change log: with its
/// number in the DOM's mutation count, only connections made before that
/// change hear it; `None` (a change made outside any run) reaches all.
pub(crate) fn fire_at(lua: &Lua, key: &SignalKey, args: &[Arg], change: Option<u64>) -> Result<()> {
    fire_args(lua, key, args, change)
}

fn fire_args(lua: &Lua, key: &SignalKey, args: &[Arg], change: Option<u64>) -> Result<()> {
    let engine = engine(lua);
    if !engine.signals.borrow().has(key) {
        return Ok(());
    }
    let values = arg::to_lua_multi(lua, &engine.ctx, args)?;
    deliver(lua, key, values, change)
}

/// [`fire`] with Luau values already in hand (`BindableEvent:Fire`).
pub(crate) fn fire_values(lua: &Lua, key: &SignalKey, args: MultiValue) -> Result<()> {
    deliver(lua, key, args, None)
}

/// [`fire_at`] with Luau values already in hand.
pub(crate) fn fire_values_at(
    lua: &Lua,
    key: &SignalKey,
    args: MultiValue,
    change: Option<u64>,
) -> Result<()> {
    deliver(lua, key, args, change)
}

fn deliver(lua: &Lua, key: &SignalKey, args: MultiValue, change: Option<u64>) -> Result<()> {
    let hears = |c: &Connection| match (change, c.since) {
        (Some(change), Some(since)) => since < change,
        _ => true,
    };
    let engine = engine(lua);
    let funcs: Vec<Function> = {
        let mut signals = engine.signals.borrow_mut();
        let Some(list) = signals.connections.get_mut(key) else {
            return Ok(());
        };
        let funcs = list
            .iter()
            .filter(|c| hears(c))
            .map(|c| c.func.clone())
            .collect();
        list.retain(|c| !(c.once && hears(c)));
        if list.is_empty() {
            signals.connections.remove(key);
        }
        funcs
    };
    for func in funcs {
        scheduler::spawn(lua, func, args.clone())?;
    }
    Ok(())
}

/// Connects `func` to `key`, stamped with the DOM's mutation count.
pub(crate) fn connect_now(lua: &Lua, key: &SignalKey, func: Function, once: bool) -> u32 {
    let engine = engine(lua);
    let since = engine.ctx.dom().mutations();
    let mut signals = engine.signals.borrow_mut();
    signals.connect(key, func, once, Some(since))
}

/// Drops every connection (plugin unload, a cancelled command-bar run).
pub(crate) fn disconnect_all(lua: &Lua) {
    engine(lua).signals.borrow_mut().connections.clear();
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::api::OutputKind;
    use crate::plugin::testing::test_vm;

    fn setup() -> (Lua, std::rc::Rc<crate::plugin::env::Env>, SignalKey) {
        let (lua, env, _) = test_vm(WeakDom::new(), false);
        let key = SignalKey::new(Owner::Plugin, "Test");
        lua.globals().set("sig", LuaSignal(key.clone())).unwrap();
        (lua, env, key)
    }

    fn run(lua: &Lua, src: &str) {
        scheduler::spawn(
            lua,
            lua.load(src).into_function().unwrap(),
            MultiValue::new(),
        )
        .unwrap();
    }

    fn num(lua: &Lua, name: &str) -> Option<f64> {
        lua.globals().get::<Option<f64>>(name).unwrap()
    }

    #[test]
    fn connect_receives_args() {
        let (lua, _env, key) = setup();
        run(&lua, "sig:Connect(function(a, b) x = a + b end)");
        fire(&lua, &key, &[Arg::Number(2.0), Arg::Number(3.0)]).unwrap();
        assert_eq!(num(&lua, "x"), Some(5.0));
        assert_eq!(
            lua.load("return tostring(sig)").eval::<String>().unwrap(),
            "Signal Test"
        );
    }

    #[test]
    fn once_runs_once() {
        let (lua, env, key) = setup();
        run(&lua, "n = 0 sig:Once(function() n = n + 1 end)");
        fire(&lua, &key, &[]).unwrap();
        fire(&lua, &key, &[]).unwrap();
        assert_eq!(num(&lua, "n"), Some(1.0));
        assert!(!env.engine.signals.borrow().has(&key));
    }

    #[test]
    fn disconnect_stops_calls() {
        let (lua, _env, key) = setup();
        run(&lua, "n = 0 c = sig:Connect(function() n = n + 1 end)");
        fire(&lua, &key, &[]).unwrap();
        run(&lua, "c:Disconnect() connected = c.Connected");
        fire(&lua, &key, &[]).unwrap();
        assert_eq!(num(&lua, "n"), Some(1.0));
        assert!(!lua.globals().get::<bool>("connected").unwrap());
    }

    #[test]
    fn wait_resumes_with_fired_args() {
        let (lua, _env, key) = setup();
        run(&lua, "got = sig:Wait()");
        assert_eq!(num(&lua, "got"), None);
        fire(&lua, &key, &[Arg::Number(7.0)]).unwrap();
        assert_eq!(num(&lua, "got"), Some(7.0));
    }

    #[test]
    fn error_in_callback_reported_others_run() {
        let (lua, env, key) = setup();
        run(
            &lua,
            "sig:Connect(function() error('bad callback') end) sig:Connect(function() ran = 1 end)",
        );
        fire(&lua, &key, &[]).unwrap();
        assert_eq!(num(&lua, "ran"), Some(1.0));
        let shared = env.shared.borrow();
        assert!(shared
            .output
            .iter()
            .any(|l| l.kind == OutputKind::Error && l.text.contains("bad callback")));
    }

    #[test]
    fn disconnect_all_clears() {
        let (lua, env, key) = setup();
        run(&lua, "sig:Connect(function() end)");
        assert!(env.engine.signals.borrow().has(&key));
        disconnect_all(&lua);
        assert!(!env.engine.signals.borrow().has(&key));
    }
}
