//! `HttpService`'s offline half, which both VMs have: JSON, GUIDs and
//! `UrlEncode`.

use mlua::{Lua, Result, Value};

/// How deep a table may nest before `JSONEncode` gives up (it would
/// otherwise recurse forever on a table that contains itself).
const MAX_DEPTH: usize = 128;

/// `HttpService[key]` for the members that need no network, `None` for
/// any other key.
pub(crate) fn member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "JSONEncode" => lua.create_function(|_, (_, value): (Value, Value)| {
            let json = to_json(&value, 0)?;
            serde_json::to_string(&json).map_err(mlua::Error::external)
        })?,
        "JSONDecode" => lua.create_function(|lua, (_, text): (Value, String)| {
            let json: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| mlua::Error::runtime(format!("Can't parse JSON: {e}")))?;
            from_json(lua, &json)
        })?,
        "GenerateGUID" => lua
            .create_function(|_, (_, wrap): (Value, Option<bool>)| Ok(guid(wrap != Some(false))))?,
        "UrlEncode" => {
            lua.create_function(|_, (_, text): (Value, String)| Ok(url_encode(&text)))?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// A random GUID in upper case, braced unless `braced` is false, as
/// `HttpService:GenerateGUID(wrapInCurlyBraces)` gives it.
fn guid(braced: bool) -> String {
    let guid = uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase();
    if braced {
        format!("{{{guid}}}")
    } else {
        guid
    }
}

fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A table with keys exactly `1..n` is an array; an empty one encodes as
/// `[]`, as Roblox does.
fn to_json(value: &Value, depth: usize) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    if depth > MAX_DEPTH {
        return Err(mlua::Error::runtime(
            "Cannot convert: table nested too deeply",
        ));
    }
    Ok(match value {
        Value::Nil => J::Null,
        Value::Boolean(b) => J::Bool(*b),
        Value::Integer(i) => J::from(*i),
        Value::Number(n) => number(*n)?,
        Value::String(s) => J::String(s.to_string_lossy().to_string()),
        Value::Table(table) => {
            let len = table.raw_len();
            let count = table.pairs::<Value, Value>().count();
            if count == len {
                let mut items = Vec::with_capacity(len);
                for i in 1..=len {
                    items.push(to_json(&table.raw_get::<Value>(i)?, depth + 1)?);
                }
                J::Array(items)
            } else {
                let mut map = serde_json::Map::new();
                for pair in table.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    let Value::String(key) = key else {
                        return Err(mlua::Error::runtime(
                            "Cannot convert table with mixed or non-string keys to JSON",
                        ));
                    };
                    map.insert(
                        key.to_string_lossy().to_string(),
                        to_json(&value, depth + 1)?,
                    );
                }
                J::Object(map)
            }
        }
        other => {
            return Err(mlua::Error::runtime(format!(
                "Can't convert {} to JSON",
                other.type_name()
            )))
        }
    })
}

fn number(n: f64) -> Result<serde_json::Value> {
    if !n.is_finite() {
        return Err(mlua::Error::runtime(
            "Can't convert a non-finite number to JSON",
        ));
    }
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Ok(serde_json::Value::from(n as i64));
    }
    serde_json::Number::from_f64(n)
        .map(serde_json::Value::Number)
        .ok_or_else(|| mlua::Error::runtime("Can't convert number to JSON"))
}

pub(crate) fn from_json(lua: &Lua, json: &serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    Ok(match json {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(items) => {
            let table = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                table.raw_set(i + 1, from_json(lua, item)?)?;
            }
            Value::Table(table)
        }
        J::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.raw_set(key.as_str(), from_json(lua, item)?)?;
            }
            Value::Table(table)
        }
    })
}
