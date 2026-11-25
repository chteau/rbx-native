//! Parallel Luau, run serially: `task.desynchronize`/`synchronize`,
//! `ConnectParallel`, the `Bind…Parallel` callbacks and `SharedTable`
//! (`parallel.luau`). A thread in the parallel phase may read anything but
//! write no property and call no mutating method, with a live server's
//! messages (the ParallelLuau golden).

use std::cell::RefCell;
use std::collections::HashMap;

use mlua::{Function, Lua, MetaMethod, Result, Table, Thread, UserData, UserDataFields, Value};
use rbx_reflection::ReflectionDatabase;

const SOURCE: &str = include_str!("parallel.luau");
const KEY: &str = "rbx_parallel";

/// The threads now in the parallel phase, held so their addresses stay
/// theirs until they synchronize.
#[derive(Default)]
struct Phase(RefCell<HashMap<usize, Thread>>);

/// Methods a live server refuses in the parallel phase, with the class
/// that declares each (what its message names).
const UNSAFE_METHODS: &[(&str, &str)] = &[
    ("SetAttribute", "Instance"),
    ("Destroy", "Instance"),
    ("ClearAllChildren", "Instance"),
    ("Remove", "Instance"),
    ("AddTag", "Instance"),
    ("RemoveTag", "Instance"),
    ("PivotTo", "PVInstance"),
    ("MoveTo", "Model"),
    ("BreakJoints", "Model"),
    ("SetPrimaryPartCFrame", "Model"),
    ("SetNetworkOwner", "BasePart"),
    ("SetNetworkOwnershipAuto", "BasePart"),
    ("ApplyImpulse", "BasePart"),
    ("FireServer", "RemoteEvent"),
    ("FireClient", "RemoteEvent"),
    ("FireAllClients", "RemoteEvent"),
    ("Fire", "BindableEvent"),
    ("Invoke", "BindableFunction"),
];

/// A SharedTable: `typeof` reads `__type` only on a real userdata, so the
/// proxy is one, its other metamethods `parallel.luau`'s.
struct SharedProxy;

impl UserData for SharedProxy {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "SharedTable");
        for name in ["__index", "__newindex", "__iter", "__tostring"] {
            fields.add_meta_field_with(name, move |lua| {
                module(lua)?.get::<Table>("proxyMeta")?.get::<Value>(name)
            });
        }
    }
}

/// Installs `task.desynchronize`/`synchronize` and the `SharedTable`
/// global. Call after the scheduler, before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    lua.set_app_data(Phase::default());
    let set = lua.create_function(|lua, on: bool| {
        let thread = lua.current_thread();
        let key = thread.to_pointer() as usize;
        if let Some(phase) = lua.app_data_ref::<Phase>() {
            let mut threads = phase.0.borrow_mut();
            if on {
                threads.insert(key, thread);
            } else {
                threads.remove(&key);
            }
        }
        Ok(())
    })?;
    let proxy = lua.create_function(|lua, ()| lua.create_userdata(SharedProxy))?;
    let module: Table = lua.load(SOURCE).set_name("=Parallel").call((set, proxy))?;
    let task: Table = lua.globals().get("task")?;
    task.set("desynchronize", module.get::<Function>("desynchronize")?)?;
    task.set("synchronize", module.get::<Function>("synchronize")?)?;
    lua.globals()
        .set("SharedTable", module.get::<Table>("SharedTable")?)?;
    lua.set_named_registry_value(KEY, module)
}

/// The module `parallel.luau` returns.
pub(crate) fn module(lua: &Lua) -> Result<Table> {
    lua.named_registry_value(KEY)
}

fn in_parallel(lua: &Lua) -> bool {
    lua.app_data_ref::<Phase>().is_some_and(|phase| {
        let threads = phase.0.borrow();
        !threads.is_empty() && threads.contains_key(&(lua.current_thread().to_pointer() as usize))
    })
}

/// `signal:ConnectParallel(handler)`: from under an Actor only, the
/// handler then run in the parallel phase.
pub(crate) fn connect_handler(lua: &Lua, handler: Function) -> Result<Function> {
    let module = module(lua)?;
    if !module.get::<Function>("underActor")?.call::<bool>(())? {
        return Err(mlua::Error::runtime(
            "Event.ConnectParallel: The calling script must be rooted under an Actor.",
        ));
    }
    module.get::<Function>("inParallel")?.call(handler)
}

/// A property write on a `class` instance, refused in the parallel phase.
pub(crate) fn check_write(
    lua: &Lua,
    db: &ReflectionDatabase,
    class: &str,
    property: &str,
) -> Result<()> {
    if !in_parallel(lua) {
        return Ok(());
    }
    let mut owner = class.to_string();
    let mut current = db.class(class);
    while let Some(descriptor) = current {
        if descriptor.properties.iter().any(|p| p.name == property) {
            owner = descriptor.name.to_string();
            break;
        }
        current = descriptor.superclass.as_deref().and_then(|s| db.class(s));
    }
    if matches!(property, "Name" | "Parent") {
        owner = "Instance".into();
    }
    Err(mlua::Error::runtime(format!(
        "Property {owner}.{property} is not safe to write in parallel"
    )))
}

/// A method looked up in the parallel phase, refused when it mutates.
pub(crate) fn check_method(lua: &Lua, method: &str) -> Result<()> {
    if !in_parallel(lua) {
        return Ok(());
    }
    match UNSAFE_METHODS.iter().find(|(name, _)| *name == method) {
        Some((name, owner)) => Err(mlua::Error::runtime(format!(
            "Function {owner}.{name} is not safe to call in parallel"
        ))),
        None => Ok(()),
    }
}
