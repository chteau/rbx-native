//! `RBXScriptSignal` / `RBXScriptConnection`.

use std::collections::HashMap;

use mlua::{Function, Lua, MultiValue, Result, UserData, UserDataMethods};
use rbx_dom::Ref;

use super::api::{Arg, UiId};

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
        SignalKey { owner, event: event.into() }
    }
}

pub(crate) struct Connection {
    pub(crate) id: u32,
    pub(crate) func: Function,
    pub(crate) once: bool,
}

/// One VM's connections.
#[derive(Default)]
pub(crate) struct Signals {
    pub(crate) connections: HashMap<SignalKey, Vec<Connection>>,
    pub(crate) next: u32,
}

impl Signals {
    pub(crate) fn has(&self, key: &SignalKey) -> bool {
        todo!()
    }

    /// Whether any connection is on an instance in `refs`' owners
    /// (any event).
    pub(crate) fn watches_instance(&self, referent: Ref) -> bool {
        todo!()
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

impl UserData for LuaSignal {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        todo!()
    }
}

impl UserData for LuaConnection {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        todo!()
    }
}

/// Fires `key` in this VM: every connected function runs in its own thread
/// (`scheduler::spawn`) with `args` converted on the VM's `Env::ctx`;
/// `Once` connections are removed before they run. A missing key is a
/// no-op.
pub(crate) fn fire(lua: &Lua, key: &SignalKey, args: &[Arg]) -> Result<()> {
    todo!()
}

/// [`fire`] with Luau values already in hand (`BindableEvent:Fire`).
pub(crate) fn fire_values(lua: &Lua, key: &SignalKey, args: MultiValue) -> Result<()> {
    todo!()
}

/// Drops every connection (plugin unload).
pub(crate) fn disconnect_all(lua: &Lua) {
    todo!()
}
