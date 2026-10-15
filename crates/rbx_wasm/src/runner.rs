//! Loading and running one WASM plugin under wasmtime.

use crate::{Effect, Grants, Host};

/// Fuel for one entry (`rbxn_start`, one `rbxn_command`): enough for real
/// work, small enough that a runaway loop ends in well under a second.
pub const FUEL_PER_CALL: u64 = 500_000_000;
/// Linear memory cap, in bytes.
pub const MEMORY_LIMIT: usize = 64 << 20;

/// One instantiated module. Not `Send`-bound: lives on the editor thread.
pub struct WasmPlugin {
    name: String,
    grants: Grants,
    // The store/instance (wasmtime `Store<State>`, `Instance`) — the
    // implementer adds the fields it needs.
    inner: Option<Box<dyn std::any::Any>>,
}

impl WasmPlugin {
    /// Compiles `bytes` (binary `.wasm` or text `.wat`), links `rbxn.call`,
    /// instantiates with fuel metering on and `MEMORY_LIMIT`. Missing
    /// `memory`/`rbxn_alloc` exports are an error.
    pub fn load(name: &str, bytes: &[u8], grants: Grants) -> Result<Self, String> {
        todo!()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Runs `rbxn_start` if exported. A trap (out of fuel included:
    /// "ran out of fuel") is an `Err`; effects pushed before it are kept.
    pub fn start(&mut self, host: &mut dyn Host) -> Result<Vec<Effect>, String> {
        todo!()
    }

    /// Runs `rbxn_command(id)` if exported.
    pub fn command(&mut self, id: &str, host: &mut dyn Host) -> Result<Vec<Effect>, String> {
        todo!()
    }
}
