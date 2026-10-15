//! Request routing and permission gating, independent of wasmtime.

use serde_json::Value;

use crate::{Effect, Grants, Host};

/// Which grant a method needs: `None` for the always-allowed ones
/// (`log`, `warn`, `notify`, `command.register`), `Err` for an unknown
/// method.
pub(crate) fn required(method: &str) -> Result<Option<&'static str>, String> {
    todo!()
}

/// Handles one raw request: parses `{"method","params"}`, checks
/// `grants`, answers `log`/`warn`/`notify`/`command.register` by pushing
/// an [`Effect`] (→ `null`), forwards everything else to `host`. Returns
/// the response JSON bytes: `{"ok": value}` or `{"error": message}` (a
/// refused permission reads `permission '<name>' not granted`).
pub(crate) fn handle(request: &[u8], grants: &Grants, host: &mut dyn Host, effects: &mut Vec<Effect>) -> Vec<u8> {
    todo!()
}

/// `params[key]` as the given JSON type, or an error naming it.
pub(crate) fn param<'a>(params: &'a Value, key: &str) -> Result<&'a Value, String> {
    todo!()
}
