//! The plugin's Luau modules in plain Luau (mlua): a stub `script` whose
//! `Parent.<Name>` evaluates the sibling module, and `require` as the
//! identity, which is all the pure modules need from Studio.

use std::path::PathBuf;

#[allow(dead_code)]
pub mod studio;

use mlua::{Lua, Table, Value};

/// One signed HTTP request to a relay at `base` (`http://host:port`), over a
/// plain socket; returns the status and the body.
#[allow(dead_code)]
pub fn signed_request(
    base: &str,
    key: &str,
    user_id: u64,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    use std::io::{Read, Write};

    let addr = base.trim_start_matches("http://").trim_end_matches('/');
    let mut stream = std::net::TcpStream::connect(addr).expect("relay is reachable");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .unwrap();
    let headers = rbx_presence::auth::headers(
        key,
        method,
        path,
        user_id,
        rbx_presence::now_ms(),
        body.as_bytes(),
        true,
    );
    let mut head = format!("{method} {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "content-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(head.as_bytes()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).expect("relay replies");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let reply = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_owned();
    (status, reply)
}

/// [`studio::stubs`] plus what the plugin's nonce needs and plain Luau lacks:
/// `Random.new()` and `HttpService:GenerateGUID()`.
#[allow(dead_code)]
pub fn studio_stubs(lua: &Lua) {
    studio::stubs(lua);
    lua.load(
        r#"
        Random = {
            new = function()
                return { NextInteger = function(_, lo, hi) return math.random(lo, hi) end }
            end,
        }
        game:GetService("HttpService").GenerateGUID = function()
            local digits = {}
            for i = 1, 32 do
                digits[i] = string.format("%x", math.random(0, 15))
            end
            return table.concat(digits)
        end
        "#,
    )
    .exec()
    .unwrap();
}

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn src_dir() -> PathBuf {
    root().join("plugins/team-presence/src")
}

/// Evaluates `<name>.luau` once and caches its returned table.
pub fn load_module(lua: &Lua, name: &str) -> mlua::Result<Table> {
    let cache: Table = lua.globals().get("__modules")?;
    if let Ok(found) = cache.get::<Table>(name) {
        return Ok(found);
    }
    let path = src_dir().join(format!("{name}.luau"));
    let src = std::fs::read_to_string(&path)
        .map_err(|e| mlua::Error::runtime(format!("{}: {e}", path.display())))?;
    let module: Table = lua.load(src).set_name(format!("={name}")).eval()?;
    cache.set(name, module.clone())?;
    Ok(module)
}

pub fn new_lua() -> Lua {
    let lua = Lua::new();
    let globals = lua.globals();
    globals
        .set("__modules", lua.create_table().unwrap())
        .unwrap();

    let parent = lua.create_table().unwrap();
    let meta = lua.create_table().unwrap();
    meta.set(
        "__index",
        lua.create_function(|lua, (_this, name): (Table, String)| load_module(lua, &name))
            .unwrap(),
    )
    .unwrap();
    parent.set_metatable(Some(meta)).unwrap();

    let script = lua.create_table().unwrap();
    script.set("Parent", parent).unwrap();
    globals.set("script", script).unwrap();
    globals
        .set(
            "require",
            lua.create_function(|_, value: Value| Ok(value)).unwrap(),
        )
        .unwrap();
    drop(globals);
    lua
}
