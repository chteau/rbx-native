//! Loading and running one WASM plugin under wasmtime.

use wasmtime::{
    Caller, Config, Engine, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, Trap, TypedFunc,
};

use crate::{abi, Effect, Grants, Host};

/// Fuel for one entry (`rbxn_start`, one `rbxn_command`): enough for real
/// work, small enough that a runaway loop ends in well under a second.
pub const FUEL_PER_CALL: u64 = 500_000_000;
/// Linear memory cap, in bytes.
pub const MEMORY_LIMIT: usize = 64 << 20;

struct State {
    limits: StoreLimits,
    grants: Grants,
    effects: Vec<Effect>,
    /// Set only while `start`/`command` runs, pointing at the caller's
    /// `&mut dyn Host`, which outlives that call; cleared before it returns.
    host: Option<*mut (dyn Host + 'static)>,
}

/// One instantiated module. Not `Send`-bound: lives on the editor thread.
pub struct WasmPlugin {
    name: String,
    store: Store<State>,
    instance: Instance,
}

fn host_call(mut caller: Caller<'_, State>, ptr: i32, len: i32) -> wasmtime::Result<i64> {
    let memory: Memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| wasmtime::Error::msg("module exports no memory"))?;
    let start = ptr as u32 as usize;
    let end = start
        .checked_add(len as u32 as usize)
        .ok_or_else(|| wasmtime::Error::msg("request out of bounds"))?;
    let request = memory
        .data(&caller)
        .get(start..end)
        .ok_or_else(|| wasmtime::Error::msg("request out of bounds"))?
        .to_vec();
    let state = caller.data_mut();
    let host_ptr = state
        .host
        .ok_or_else(|| wasmtime::Error::msg("rbxn.call outside an entry point"))?;
    // SAFETY: `host` is only set for the duration of `WasmPlugin::enter`,
    // whose `&mut dyn Host` borrow is exclusive and alive throughout.
    let host = unsafe { &mut *host_ptr };
    let response = abi::handle(&request, &state.grants, host, &mut state.effects);
    let alloc: TypedFunc<i32, i32> = caller
        .get_export("rbxn_alloc")
        .and_then(|e| e.into_func())
        .ok_or_else(|| wasmtime::Error::msg("module exports no rbxn_alloc"))?
        .typed(&caller)?;
    let out = alloc.call(&mut caller, response.len() as i32)?;
    memory.write(&mut caller, out as u32 as usize, &response)?;
    Ok(((out as u32 as i64) << 32) | response.len() as i64)
}

fn describe(error: wasmtime::Error) -> String {
    match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "ran out of fuel".to_string(),
        _ => format!("{error:#}"),
    }
}

impl WasmPlugin {
    /// Compiles `bytes` (binary `.wasm` or text `.wat`), links `rbxn.call`,
    /// instantiates with fuel metering on and `MEMORY_LIMIT`. Missing
    /// `memory`/`rbxn_alloc` exports are an error.
    pub fn load(name: &str, bytes: &[u8], grants: Grants) -> Result<Self, String> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(describe)?;
        let module = Module::new(&engine, bytes).map_err(describe)?;
        let state = State {
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
            grants,
            effects: Vec::new(),
            host: None,
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_PER_CALL).map_err(describe)?;
        let mut linker = Linker::new(&engine);
        linker
            .func_wrap("rbxn", "call", host_call)
            .map_err(describe)?;
        let instance = linker.instantiate(&mut store, &module).map_err(describe)?;
        if instance.get_memory(&mut store, "memory").is_none() {
            return Err(format!("{name}: module does not export 'memory'"));
        }
        if instance
            .get_typed_func::<i32, i32>(&mut store, "rbxn_alloc")
            .is_err()
        {
            return Err(format!(
                "{name}: module does not export 'rbxn_alloc(i32) -> i32'"
            ));
        }
        Ok(Self {
            name: name.to_string(),
            store,
            instance,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn enter(
        &mut self,
        host: &mut dyn Host,
        run: impl FnOnce(&mut Store<State>, &Instance) -> wasmtime::Result<()>,
    ) -> Result<Vec<Effect>, String> {
        // SAFETY: erases the borrow's lifetime only; the pointer is cleared
        // below before `host`'s borrow ends.
        let raw: *mut (dyn Host + 'static) = unsafe { std::mem::transmute(host as *mut dyn Host) };
        self.store.data_mut().host = Some(raw);
        let result = self
            .store
            .set_fuel(FUEL_PER_CALL)
            .and_then(|()| run(&mut self.store, &self.instance));
        let state = self.store.data_mut();
        state.host = None;
        let effects = std::mem::take(&mut state.effects);
        result.map(|()| effects).map_err(describe)
    }

    /// Runs `rbxn_start` if exported. A trap (out of fuel included:
    /// "ran out of fuel") is an `Err`; effects pushed before it are kept.
    pub fn start(&mut self, host: &mut dyn Host) -> Result<Vec<Effect>, String> {
        self.enter(host, |store, instance| {
            match instance.get_typed_func::<(), ()>(&mut *store, "rbxn_start") {
                Ok(start) => start.call(store, ()),
                Err(_) => Ok(()),
            }
        })
    }

    /// Runs `rbxn_command(id)` if exported.
    pub fn command(&mut self, id: &str, host: &mut dyn Host) -> Result<Vec<Effect>, String> {
        self.enter(host, |store, instance| {
            let Ok(command) =
                instance.get_typed_func::<(i32, i32), ()>(&mut *store, "rbxn_command")
            else {
                return Ok(());
            };
            let alloc = instance.get_typed_func::<i32, i32>(&mut *store, "rbxn_alloc")?;
            let len = id.len() as i32;
            let ptr = alloc.call(&mut *store, len)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| wasmtime::Error::msg("module exports no memory"))?;
            memory.write(&mut *store, ptr as u32 as usize, id.as_bytes())?;
            command.call(store, (ptr, len))
        })
    }
}
