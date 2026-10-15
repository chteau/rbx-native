//! Studio's `HttpService` and `DateTime`, stood in for so the plugin's own
//! `Relay.luau` can talk to a local relay in the tests.

/// `game:GetService("HttpService")` with `RequestAsync` done over a plain
/// HTTP/1.1 socket (the relay under test is local), `JSONDecode` through the
/// plugin's own `Json`, and `DateTime.now()`.
pub fn stubs(lua: &mlua::Lua) {
    let request = lua
        .create_function(|lua, (_, request): (mlua::Table, mlua::Table)| {
            let url: String = request.get("Url")?;
            let method: String = request.get("Method")?;
            let body: Option<String> = request.get("Body")?;
            // Optional, as in Studio.
            let headers: Option<mlua::Table> = request.get("Headers")?;
            let mut extra = String::new();
            for pair in headers.iter().flat_map(|h| h.pairs::<String, String>()) {
                let (name, value) = pair?;
                extra.push_str(&format!("{name}: {value}\r\n"));
            }
            let (status, text) = http(&url, &method, &extra, body.as_deref().unwrap_or(""))
                .map_err(mlua::Error::runtime)?;
            let response = lua.create_table()?;
            response.set("Success", (200..300).contains(&status))?;
            response.set("StatusCode", status)?;
            response.set("Body", text)?;
            Ok(response)
        })
        .unwrap();
    let setup = lua
        .load(
            r#"
            local requestAsync, nowMs = ...
            local http = {
                RequestAsync = requestAsync,
                JSONDecode = function(_, text) return script.Parent.Json.decode(text) end,
            }
            game = { GetService = function(_, name) assert(name == "HttpService"); return http end }
            DateTime = { now = function() return { UnixTimestampMillis = nowMs() } end }
            "#,
        )
        .into_function()
        .unwrap();
    let now_ms = lua
        .create_function(|_, ()| {
            let since = std::time::UNIX_EPOCH.elapsed().expect("clock after 1970");
            Ok(since.as_millis() as f64)
        })
        .unwrap();
    setup.call::<()>((request, now_ms)).unwrap();
}

/// One request; answers the status and the (de-chunked) body.
pub fn http(url: &str, method: &str, headers: &str, body: &str) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").ok_or("http:// only")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let mut socket = std::net::TcpStream::connect(host).map_err(|e| e.to_string())?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n",
        body.len()
    );
    socket
        .write_all(format!("{head}{body}").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut raw = String::new();
    socket.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let (head, mut payload) = raw.split_once("\r\n\r\n").ok_or("no header end")?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("no status")?;
    let mut body = String::new();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        while let Some((size, rest)) = payload.split_once("\r\n") {
            let size = usize::from_str_radix(size.trim(), 16).map_err(|e| e.to_string())?;
            if size == 0 {
                break;
            }
            body.push_str(&rest[..size]);
            payload = &rest[size + 2..];
        }
    } else {
        body.push_str(payload);
    }
    Ok((status, body))
}
