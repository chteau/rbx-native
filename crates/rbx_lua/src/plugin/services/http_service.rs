//! `HttpService`: JSON, GUIDs and the permission-gated, yielding requests.

use mlua::{Function, Lua, MultiValue, Result, Table, Value};

use super::state;
use crate::plugin::api::{Answer, Permission, PromptKind, Request, UiId};
use crate::plugin::env::env;
use crate::plugin::http::{host_of, Finished, HttpRequest};
use crate::plugin::native;
use crate::plugin::scheduler::{self, AwaitId};

const PERMISSION_KEY: &str = "__rbxnative_http";
const MAX_DEPTH: usize = 128;

/// What a pending request's script wants back.
#[derive(Clone, Copy)]
pub(super) enum Mode {
    /// `RequestAsync`: the response table.
    Response,
    /// `GetAsync`/`PostAsync`: the body, an error unless 2xx.
    Body,
}

fn denied(host: &str) -> String {
    format!("Http requests to {host} are not allowed")
}

pub(super) fn index(lua: &Lua, key: &str) -> Result<Value> {
    Ok(match key {
        "HttpEnabled" => Value::Boolean(true),
        "JSONEncode" => func(lua, |_, (_, value): (Value, Value)| {
            let json = to_json(&value, 0)?;
            serde_json::to_string(&json).map_err(mlua::Error::external)
        })?,
        "JSONDecode" => func(lua, |lua, (_, text): (Value, String)| {
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| mlua::Error::runtime(format!("Can't parse JSON: {e}")))?;
            from_json(lua, &json)
        })?,
        "GenerateGUID" => func(lua, |_, (_, wrap): (Value, Option<bool>)| {
            let guid = uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase();
            Ok(if wrap == Some(false) { guid } else { format!("{{{guid}}}") })
        })?,
        "UrlEncode" => func(lua, |_, (_, text): (Value, String)| Ok(url_encode(&text)))?,
        "RequestAsync" => yielding(lua, |lua, (_, options): (Value, Table)| {
            let url: String = options.get("Url")?;
            let method: Option<String> = options.get("Method")?;
            let body: Option<String> = options.get("Body")?;
            let mut headers = Vec::new();
            if let Some(table) = options.get::<Option<Table>>("Headers")? {
                for pair in table.pairs::<String, String>() {
                    headers.push(pair?);
                }
            }
            let request = HttpRequest {
                url,
                method: method.unwrap_or_else(|| "GET".into()).to_ascii_uppercase(),
                headers,
                body,
            };
            begin(lua, request, Mode::Response)
        })?,
        "GetAsync" => yielding(lua, |lua, (_, url): (Value, String)| {
            let request = HttpRequest { url, method: "GET".into(), headers: Vec::new(), body: None };
            begin(lua, request, Mode::Body)
        })?,
        "PostAsync" => yielding(lua, |lua, (_, url, body, content_type): (Value, String, String, Option<String>)| {
            let headers = vec![("Content-Type".to_string(), content_type.unwrap_or_else(|| "application/json".into()))];
            let request = HttpRequest { url, method: "POST".into(), headers, body: Some(body) };
            begin(lua, request, Mode::Body)
        })?,
        _ => return Err(mlua::Error::runtime(format!("{key} is not a valid member of HttpService"))),
    })
}

fn func<A, R>(lua: &Lua, f: impl Fn(&Lua, A) -> Result<R> + 'static) -> Result<Value>
where
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    Ok(Value::Function(lua.create_function(f)?))
}

fn yielding<A: mlua::FromLuaMulti>(lua: &Lua, start: impl Fn(&Lua, A) -> Result<AwaitId> + 'static) -> Result<Value> {
    let start: Function = lua.create_function(start)?;
    Ok(Value::Function(scheduler::yielding(lua, start)?))
}

/// Checks the plugin may talk to the request's host, then sends it, or
/// parks it behind a permission prompt.
fn begin(lua: &Lua, request: HttpRequest, mode: Mode) -> Result<AwaitId> {
    let host = host_of(&request.url)
        .ok_or_else(|| mlua::Error::runtime(format!("HttpService: invalid URL \"{}\"", request.url)))?;
    let env = env(lua);
    let id = scheduler::new_await(lua);
    state(lua).borrow_mut().modes.insert(id, mode);

    let decision = if env.manifest.is_some() {
        Some(native::allowed(lua, &Permission::Http(host.clone())))
    } else {
        env.shared
            .borrow()
            .settings
            .get(&env.name)
            .and_then(|s| s.get(PERMISSION_KEY))
            .and_then(|m| m.get(&host))
            .and_then(|v| v.as_bool())
    };
    match decision {
        Some(true) => env.shared.borrow_mut().http.start(env.plugin, id, request),
        Some(false) => {
            state(lua).borrow_mut().modes.remove(&id);
            return Err(mlua::Error::runtime(denied(&host)));
        }
        None => {
            let state = state(lua);
            let mut state = state.borrow_mut();
            let waiting = state.prompts.iter_mut().find(|(_, (h, _))| *h == host);
            match waiting {
                Some((_, (_, list))) => list.push((id, request)),
                None => {
                    let mut shared = env.shared.borrow_mut();
                    let prompt = shared.next_id();
                    shared.requests.push(Request::Prompt {
                        id: prompt,
                        plugin: env.plugin,
                        kind: PromptKind::HttpPermission { host: host.clone() },
                    });
                    state.prompts.insert(prompt, (host, vec![(id, request)]));
                }
            }
        }
    }
    Ok(id)
}

pub(super) fn permission_answered(lua: &Lua, prompt: UiId, answer: &Answer) -> Result<()> {
    let Answer::Allow(allow) = answer else {
        return Ok(());
    };
    let Some((host, waiting)) = state(lua).borrow_mut().prompts.remove(&prompt) else {
        return Ok(());
    };
    let env = env(lua);
    {
        let mut shared = env.shared.borrow_mut();
        let hosts = shared
            .settings
            .entry(env.name.clone())
            .or_default()
            .entry(PERMISSION_KEY.to_string())
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let Some(map) = hosts.as_object_mut() {
            map.insert(host.clone(), serde_json::Value::Bool(*allow));
        }
        save_settings(&mut shared, &env.name);
    }
    for (id, request) in waiting {
        if *allow {
            env.shared.borrow_mut().http.start(env.plugin, id, request);
        } else {
            state(lua).borrow_mut().modes.remove(&id);
            scheduler::resolve(lua, id, Err(denied(&host)))?;
        }
    }
    Ok(())
}

fn save_settings(shared: &mut crate::plugin::env::Shared, name: &str) {
    let (Some(dir), Some(map)) = (shared.settings_dir.clone(), shared.settings.get(name)) else {
        return;
    };
    let file = dir.join(format!("{}.json", name.replace(['/', '\\'], "_")));
    let written = serde_json::to_string_pretty(map)
        .map_err(|e| e.to_string())
        .and_then(|text| {
            std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(&file, text))
                .map_err(|e| e.to_string())
        });
    if let Err(error) = written {
        shared.push_output(
            name,
            crate::plugin::api::OutputKind::Warn,
            format!("could not save settings: {error}"),
        );
    }
}

pub(super) fn finished(lua: &Lua, finished: Vec<Finished>) -> Result<()> {
    let plugin = env(lua).plugin;
    for (owner, id, result) in finished {
        if owner != plugin {
            continue;
        }
        let mode = state(lua).borrow_mut().modes.remove(&id).unwrap_or(Mode::Response);
        let outcome = match (result, mode) {
            (Err(message), _) => Err(format!("HttpService: {message}")),
            (Ok(response), Mode::Response) => {
                let table = lua.create_table()?;
                table.set("Success", response.success)?;
                table.set("StatusCode", response.status_code)?;
                table.set("StatusMessage", response.status_message)?;
                let headers = lua.create_table()?;
                for (name, value) in response.headers {
                    headers.set(name, value)?;
                }
                table.set("Headers", headers)?;
                table.set("Body", response.body)?;
                Ok(MultiValue::from_vec(vec![Value::Table(table)]))
            }
            (Ok(response), Mode::Body) if response.success => {
                Ok(MultiValue::from_vec(vec![Value::String(lua.create_string(&response.body)?)]))
            }
            (Ok(response), Mode::Body) => Err(format!(
                "HTTP {} ({})",
                response.status_code, response.status_message
            )),
        };
        scheduler::resolve(lua, id, outcome)?;
    }
    Ok(())
}

fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
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
        return Err(mlua::Error::runtime("Cannot convert: table nested too deeply"));
    }
    Ok(match value {
        Value::Nil => J::Null,
        Value::Boolean(b) => J::Bool(*b),
        Value::Integer(i) => J::from(*i as i64),
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
                        return Err(mlua::Error::runtime("Cannot convert table with mixed or non-string keys to JSON"));
                    };
                    map.insert(key.to_string_lossy().to_string(), to_json(&value, depth + 1)?);
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
        return Err(mlua::Error::runtime("Can't convert a non-finite number to JSON"));
    }
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Ok(serde_json::Value::from(n as i64));
    }
    serde_json::Number::from_f64(n)
        .map(serde_json::Value::Number)
        .ok_or_else(|| mlua::Error::runtime("Can't convert number to JSON"))
}

fn from_json(lua: &Lua, json: &serde_json::Value) -> Result<Value> {
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
