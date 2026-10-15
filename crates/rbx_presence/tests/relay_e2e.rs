//! Two editors meeting in a real relay. Needs one running
//! (`cd relay && npx wrangler dev`), so it is ignored unless asked for:
//! `RBX_PRESENCE_RELAY=http://127.0.0.1:8787 cargo test -p rbx_presence --test relay_e2e -- --ignored`.

mod common;

use std::time::{Duration, Instant};

use rbx_presence::client::{Client, Config, Event};
use rbx_presence::schema::{Client as Editor, UserState, SCHEMA_VERSION};
use rbx_presence::{auth, secret};

fn state(user_id: u64, selection: &str) -> UserState {
    UserState {
        v: SCHEMA_VERSION,
        user_id,
        name: format!("User {user_id}"),
        color: rbx_presence::schema::color_for(user_id).to_owned(),
        client: Editor::RbxNative,
        camera: None,
        selection: vec![selection.to_owned()],
        script: None,
        edits: Vec::new(),
        seen: 0,
    }
}

fn join(relay: &str, key: &str, user_id: u64) -> (Client, std::sync::mpsc::Receiver<Event>) {
    Client::start(Config {
        relay_url: relay.to_owned(),
        room: auth::room_id(key),
        key: key.to_owned(),
        user_id,
    })
}

/// Waits until `events` reports a snapshot in which `other` has `selection`.
fn sees(events: &std::sync::mpsc::Receiver<Event>, other: u64, selection: &str) -> bool {
    let end = Instant::now() + Duration::from_secs(15);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(Event::Snapshot(snapshot)) => {
                let found = snapshot
                    .users
                    .iter()
                    .any(|user| user.user_id == other && user.selection == [selection]);
                if found {
                    return true;
                }
            }
            Ok(Event::Rejected(why)) => panic!("relay refused: {why}"),
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[test]
#[ignore = "needs a running relay in RBX_PRESENCE_RELAY"]
fn two_editors_see_each_other_and_a_stranger_is_refused() {
    let relay = std::env::var("RBX_PRESENCE_RELAY").expect("RBX_PRESENCE_RELAY");
    let key = auth::derive_key(&secret::generate(), 4567);

    let (ana, ana_events) = join(&relay, &key, 1);
    let (ben, ben_events) = join(&relay, &key, 2);
    ana.publish(state(1, "Workspace.Baseplate"));
    ben.publish(state(2, "Workspace.SpawnLocation"));
    assert!(sees(&ana_events, 2, "Workspace.SpawnLocation"));
    assert!(sees(&ben_events, 1, "Workspace.Baseplate"));

    // Someone who knows the room id but not the key: their own key does
    // not hash to that room, so the relay turns them away.
    let (_eve, eve_events) = Client::start(Config {
        relay_url: relay.clone(),
        room: auth::room_id(&key),
        key: auth::derive_key(&secret::generate(), 4567),
        user_id: 3,
    });
    let refused = loop {
        match eve_events.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Rejected(_)) => break true,
            Ok(Event::Connected) | Err(_) => break false,
            Ok(_) => {}
        }
    };
    assert!(refused);
}

/// The plugin's own `Relay.luau`, over a stand-in for `HttpService`, meets
/// this crate's client in the same room.
#[test]
#[ignore = "needs a running relay in RBX_PRESENCE_RELAY"]
fn the_plugin_and_the_editor_see_each_other() {
    let relay = std::env::var("RBX_PRESENCE_RELAY").expect("RBX_PRESENCE_RELAY");
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);

    let lua = common::new_lua();
    studio_stubs(&lua);
    let (editor, editor_events) = join(&relay, &key, 8);
    editor.publish(state(8, "Workspace.SpawnLocation"));

    let chunk = format!(
        r##"
        local Auth = script.Parent.Auth
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{secret}", 4567)
        local relay = Relay.new({{ url = "{relay}", key = key, room = Auth.roomId(key), userId = 7 }})
        local ok, err = relay:publish({{
            v = 1, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
            selection = {{ "Workspace.Baseplate" }}, edits = {{}}, seen = 0,
        }})
        assert(ok, err)
        local cursor = 0
        for _ = 1, 10 do
            local snapshot, pollErr = relay:poll(cursor)
            assert(snapshot, pollErr)
            cursor = snapshot.cursor
            for _, user in snapshot.users do
                if user.userId == 8 and user.selection[1] == "Workspace.SpawnLocation" then
                    relay:leave()
                    return true
                end
            end
        end
        return false
        "##
    );
    let plugin_saw_editor: bool = lua.load(chunk).eval().expect("plugin side ran");
    assert!(plugin_saw_editor);
    assert!(sees(&editor_events, 7, "Workspace.Baseplate"));
}

/// `game:GetService("HttpService")` with `RequestAsync` done over a plain
/// HTTP/1.1 socket (the relay under test is local), `JSONDecode` through the
/// plugin's own `Json`, and `DateTime.now()`.
fn studio_stubs(lua: &mlua::Lua) {
    let request = lua
        .create_function(|lua, (_, request): (mlua::Table, mlua::Table)| {
            let url: String = request.get("Url")?;
            let method: String = request.get("Method")?;
            let body: Option<String> = request.get("Body")?;
            let headers: mlua::Table = request.get("Headers")?;
            let mut extra = String::new();
            for pair in headers.pairs::<String, String>() {
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
fn http(url: &str, method: &str, headers: &str, body: &str) -> Result<(u16, String), String> {
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
