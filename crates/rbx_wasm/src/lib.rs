//! rbx-native's own WASM plugins: a module described by a `plugin.toml`,
//! run under `wasmtime` with a fuel budget and a memory cap, reaching the
//! editor only through one JSON host call whose methods are gated by the
//! manifest's permissions.
//!
//! # ABI
//!
//! The module exports `memory`, `rbxn_alloc(len: i32) -> i32` (a buffer the
//! host may write `len` bytes into), and optionally `rbxn_start()` (run
//! once after loading) and `rbxn_command(ptr: i32, len: i32)` (a registered
//! command was clicked; the bytes are its id, UTF-8).
//!
//! It may import `rbxn.call(ptr: i32, len: i32) -> i64`: the bytes at
//! `ptr..ptr+len` are a JSON request `{"method": "...", "params": {...}}`;
//! the host answers with `{"ok": value}` or `{"error": "message"}`, written
//! into a buffer from `rbxn_alloc`, and returns `(ptr << 32) | len`.
//!
//! Methods (permission in brackets):
//! - `log {text}` / `warn {text}` — Output lines (none);
//! - `notify {text}` — a toast (none);
//! - `command.register {id, text, tooltip?}` — a Plugins-tab button (none);
//! - `selection.get` → `[ref…]`, `selection.set {refs}` [`selection`];
//! - `instance.roots` → `[ref…]`, `instance.get {ref}` →
//!   `{ref, name, class, parent, children}`, `instance.find {path}`
//!   (`"Workspace.Model.Part"`) → ref or null,
//!   `instance.property {ref, name}` → value [`dom.read`];
//! - `instance.set_property {ref, name, value}`, `instance.new {class,
//!   parent?, name?}` → ref, `instance.set_parent {ref, parent}`,
//!   `instance.destroy {ref}` [`dom.write`].
//!
//! Refs are JSON numbers (the DOM's referent value). Property values:
//! bool, number, string, `{"x","y","z"}` Vector3, `{"x","y"}` Vector2,
//! `{"r","g","b"}` Color3 (0..1), `{"position":[3],"rotation":[9]}`
//! CFrame, `{"enum": n}` an enum value, `{"ref": n}` a reference; other
//! types read as `{"unsupported": "<type>"}` and cannot be written.

mod abi;
mod dom_host;
mod runner;

pub use dom_host::DomHost;
pub use runner::{WasmPlugin, FUEL_PER_CALL, MEMORY_LIMIT};

/// What a WASM plugin may do (from its `plugin.toml` permissions, as
/// strings: `dom.read`, `dom.write`, `selection`, `debugger`,
/// `http:<host>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grants {
    pub dom_read: bool,
    pub dom_write: bool,
    pub selection: bool,
}

impl Grants {
    /// From manifest permission strings; unknown ones are ignored here
    /// (the manifest parser already rejected them).
    pub fn from_permissions<S: AsRef<str>>(permissions: &[S]) -> Self {
        let mut grants = Grants::default();
        for permission in permissions {
            match permission.as_ref() {
                "dom.read" => grants.dom_read = true,
                "dom.write" => grants.dom_write = true,
                "selection" => grants.selection = true,
                _ => {}
            }
        }
        grants
    }
}

/// What the editor provides to a WASM plugin. `method`/`params` are the
/// request's; permission checks already passed when this is called.
pub trait Host {
    fn call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// Something a plugin asked for that only the editor can show.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Log(String),
    Warn(String),
    Notify(String),
    Command {
        id: String,
        text: String,
        tooltip: String,
    },
}
