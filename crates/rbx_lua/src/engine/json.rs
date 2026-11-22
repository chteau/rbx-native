//! `HttpService`'s offline half, which both VMs have: JSON, GUIDs and
//! `UrlEncode`, written to give the text a live server gives (the HttpJson
//! golden).

use mlua::{Lua, Result, Table, Value};

/// How deep tables may nest before `JSONEncode` gives up with Luau's own
/// stack message (a live server fails somewhere under 600).
const MAX_DEPTH: usize = 200;

/// `HttpService[key]` for the members that need no network, `None` for
/// any other key.
pub(crate) fn member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "JSONEncode" => lua.create_function(|_, (_, value): (Value, Value)| encode(&value))?,
        "JSONDecode" => lua.create_function(|lua, (_, text): (Value, Value)| {
            let text = string_arg(&text, 1)?;
            decode(lua, &text)
        })?,
        "GenerateGUID" => lua.create_function(|lua, (_, wrap): (Value, Option<bool>)| {
            Ok(guid(lua, wrap != Some(false)))
        })?,
        "UrlEncode" => lua.create_function(|_, (_, text): (Value, Value)| {
            Ok(url_encode(&string_arg(&text, 1)?))
        })?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// A string argument as Roblox casts one: numbers become their text, nil
/// is "Argument N missing or nil".
pub(crate) fn string_arg(value: &Value, index: usize) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy().to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(number(*n)),
        Value::Nil => Err(mlua::Error::runtime(format!(
            "Argument {index} missing or nil"
        ))),
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to string",
            other.type_name()
        ))),
    }
}

/// A random version 4 GUID in lower case, braced unless `braced` is
/// false, as `HttpService:GenerateGUID(wrapInCurlyBraces)` gives it.
pub(crate) fn guid(lua: &Lua, braced: bool) -> String {
    let high = crate::tape::entropy(lua, rand_u64);
    let low = crate::tape::entropy(lua, rand_u64);
    let bits = (u128::from(high) << 64 | u128::from(low)) & !(0xF000 << 64 | 0xC << 60)
        | 0x4000 << 64
        | 0x8 << 60;
    let guid = uuid::Uuid::from_u128(bits).hyphenated().to_string();
    if braced {
        format!("{{{guid}}}")
    } else {
        guid
    }
}

fn rand_u64() -> u64 {
    let (high, _) = uuid::Uuid::new_v4().as_u64_pair();
    high
}

/// Every byte but ASCII letters and digits as `%XX`.
fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `JSONEncode`: a table whose length (`#t`) is above zero is an array of
/// `t[1..#t]` (holes are `null`, other keys are dropped), an empty table
/// is `[]`, any other is an object in `pairs` order (number keys as their
/// integer part). Values JSON has no type for (userdata, functions) are
/// `null`; non-finite numbers are Roblox's `{"m":null,"t":"numeric",...}`.
pub(crate) fn encode(value: &Value) -> Result<String> {
    let mut out = String::new();
    let mut seen = Vec::new();
    write(&mut out, value, &mut seen)?;
    Ok(out)
}

fn write(out: &mut String, value: &Value, seen: &mut Vec<Table>) -> Result<()> {
    match value {
        Value::Nil => out.push_str("null"),
        Value::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Integer(i) => out.push_str(&i.to_string()),
        Value::Number(n) if n.is_finite() => out.push_str(&number(*n)),
        Value::Number(n) => {
            let v = if n.is_nan() {
                "nan"
            } else if *n > 0.0 {
                "inf"
            } else {
                "-inf"
            };
            out.push_str(&format!(r#"{{"m":null,"t":"numeric","v":"{v}"}}"#));
        }
        Value::String(s) => {
            let bytes = s.as_bytes();
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| mlua::Error::runtime("Can't convert to JSON"))?;
            string(out, text);
        }
        Value::Table(table) => {
            if seen.iter().any(|t| t == table) {
                return Err(mlua::Error::runtime("tables cannot be cyclic"));
            }
            if seen.len() >= MAX_DEPTH {
                return Err(mlua::Error::runtime("Max lua stack size reached"));
            }
            seen.push(table.clone());
            let len = table.raw_len();
            if len > 0 {
                out.push('[');
                for i in 1..=len {
                    if i > 1 {
                        out.push(',');
                    }
                    write(out, &table.raw_get::<Value>(i)?, seen)?;
                }
                out.push(']');
            } else {
                let mut first = true;
                for pair in table.pairs::<Value, Value>() {
                    let (key, item) = pair?;
                    let key = match key {
                        Value::String(s) => s.to_string_lossy().to_string(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) => (n.trunc() as i64).to_string(),
                        _ => return Err(mlua::Error::runtime("Invalid table key type used")),
                    };
                    out.push(if first { '{' } else { ',' });
                    first = false;
                    string(out, &key);
                    out.push(':');
                    write(out, &item, seen)?;
                }
                out.push_str(if first { "[]" } else { "}" });
            }
            seen.pop();
        }
        _ => out.push_str("null"),
    }
    Ok(())
}

/// A JSON string: quotes, backslashes and the named control escapes, other
/// control characters as `\u00XX`; everything else (DEL, `/`, U+2028, any
/// other Unicode) as it is.
fn string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A finite number in its shortest round-trip digits, laid out as
/// JavaScript does (plain from 1e-7 up to 1e21, else `1.5e300`), but with
/// no `+` in the exponent.
pub(crate) fn number(n: f64) -> String {
    if n == 0.0 {
        return if n.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sci = format!("{n:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    let point = exp + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if k == 1 {
        format!("{digits}e{}", point - 1)
    } else {
        format!("{}.{}e{}", &digits[..1], &digits[1..], point - 1)
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// `JSONDecode`: `null` in an array leaves a hole, in an object drops the
/// key; Roblox's non-finite number objects come back as numbers.
pub(crate) fn decode(lua: &Lua, text: &str) -> Result<Value> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|_| mlua::Error::runtime("Can't parse JSON"))?;
    from_json(lua, &json)
}

pub(crate) fn from_json(lua: &Lua, json: &serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    Ok(match json {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(items) => {
            // Sized up front, so `#t` counts a trailing value after holes.
            let table = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                table.raw_set(i + 1, from_json(lua, item)?)?;
            }
            Value::Table(table)
        }
        J::Object(map) => {
            if let Some(n) = non_finite(map) {
                return Ok(Value::Number(n));
            }
            let table = lua.create_table()?;
            for (key, item) in map {
                table.raw_set(key.as_str(), from_json(lua, item)?)?;
            }
            Value::Table(table)
        }
    })
}

fn non_finite(map: &serde_json::Map<String, serde_json::Value>) -> Option<f64> {
    if map.len() != 3 || !map.get("m")?.is_null() || map.get("t")?.as_str()? != "numeric" {
        return None;
    }
    match map.get("v")?.as_str()? {
        "nan" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn numbers_print_as_a_live_server_prints_them() {
        for (n, text) in [
            (0.1, "0.1"),
            (1.0 / 3.0, "0.3333333333333333"),
            (1e20, "100000000000000000000"),
            (1e21, "1e21"),
            (1e300, "1e300"),
            (1e-5, "0.00001"),
            (1e-7, "1e-7"),
            (2f64.powi(32) + 0.5, "4294967296.5"),
            (-7.0, "-7"),
            (-0.0, "-0"),
            (19.99, "19.99"),
            (1.5e-9, "1.5e-9"),
        ] {
            assert_eq!(number(n), text, "{n}");
        }
    }
}
