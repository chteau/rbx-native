//! Request routing and permission gating, independent of wasmtime.

use serde_json::{json, Value};

use crate::{Effect, Grants, Host};

/// Which grant a method needs: `None` for the always-allowed ones
/// (`log`, `warn`, `notify`, `command.register`), `Err` for an unknown
/// method.
pub(crate) fn required(method: &str) -> Result<Option<&'static str>, String> {
    match method {
        "log" | "warn" | "notify" | "command.register" => Ok(None),
        "selection.get" | "selection.set" => Ok(Some("selection")),
        "instance.roots" | "instance.get" | "instance.find" | "instance.property" => Ok(Some("dom.read")),
        "instance.set_property" | "instance.new" | "instance.set_parent" | "instance.destroy" => {
            Ok(Some("dom.write"))
        }
        other => Err(format!("unknown method '{other}'")),
    }
}

fn granted(grants: &Grants, permission: &str) -> bool {
    match permission {
        "selection" => grants.selection,
        "dom.read" => grants.dom_read,
        "dom.write" => grants.dom_write,
        _ => false,
    }
}

fn string_param(params: &Value, key: &str) -> Result<String, String> {
    param(params, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("param '{key}' must be a string"))
}

fn dispatch(request: &[u8], grants: &Grants, host: &mut dyn Host, effects: &mut Vec<Effect>) -> Result<Value, String> {
    let request: Value = serde_json::from_slice(request).map_err(|e| format!("invalid request: {e}"))?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or("request has no 'method' string")?;
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    if let Some(permission) = required(method)? {
        if !granted(grants, permission) {
            return Err(format!("permission '{permission}' not granted"));
        }
        return host.call(method, &params);
    }
    let effect = match method {
        "log" => Effect::Log(string_param(&params, "text")?),
        "warn" => Effect::Warn(string_param(&params, "text")?),
        "notify" => Effect::Notify(string_param(&params, "text")?),
        _ => Effect::Command {
            id: string_param(&params, "id")?,
            text: string_param(&params, "text")?,
            tooltip: match params.get("tooltip") {
                None | Some(Value::Null) => String::new(),
                Some(_) => string_param(&params, "tooltip")?,
            },
        },
    };
    effects.push(effect);
    Ok(Value::Null)
}

/// Handles one raw request: parses `{"method","params"}`, checks
/// `grants`, answers `log`/`warn`/`notify`/`command.register` by pushing
/// an [`Effect`] (→ `null`), forwards everything else to `host`. Returns
/// the response JSON bytes: `{"ok": value}` or `{"error": message}` (a
/// refused permission reads `permission '<name>' not granted`).
pub(crate) fn handle(request: &[u8], grants: &Grants, host: &mut dyn Host, effects: &mut Vec<Effect>) -> Vec<u8> {
    let response = match dispatch(request, grants, host, effects) {
        Ok(value) => json!({ "ok": value }),
        Err(message) => json!({ "error": message }),
    };
    response.to_string().into_bytes()
}

/// `params[key]` as the given JSON type, or an error naming it.
pub(crate) fn param<'a>(params: &'a Value, key: &str) -> Result<&'a Value, String> {
    params.get(key).ok_or_else(|| format!("missing param '{key}'"))
}
