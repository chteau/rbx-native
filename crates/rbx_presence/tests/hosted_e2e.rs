//! A hosted room end to end, on this machine: a stand-in ntfy server, one
//! rbx-native client allowed to host (its "tunnel" is the loopback port
//! itself) and one that only joins. The first hosts and announces, the
//! second finds the announcement and joins, they see each other, and the
//! host says goodbye when it stops.

mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rbx_presence::client::{Client, Config, Event, Hosting, Route, Tunnel};
use rbx_presence::relay::RelayKind;
use rbx_presence::schema::{Client as Editor, UserState, SCHEMA_VERSION};
use rbx_presence::{announce, auth, crypt, secret};

type Topics = Arc<Mutex<HashMap<String, Vec<String>>>>;

/// `POST /<topic>` stores the body; `GET /<topic>/json` answers every stored
/// message as ntfy's JSON lines.
fn fake_ntfy() -> (String, Topics) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let topics: Topics = Arc::default();
    let store = Arc::clone(&topics);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let store = Arc::clone(&store);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_owned();
                let target = parts.next().unwrap_or("").to_owned();
                let mut len = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let path = target.split('?').next().unwrap().trim_start_matches('/');
                let reply = if method == "POST" {
                    let mut topics = store.lock().unwrap();
                    topics
                        .entry(path.to_owned())
                        .or_default()
                        .push(String::from_utf8(body).unwrap());
                    "{}".to_owned()
                } else {
                    let topic = path.trim_end_matches("/json");
                    let topics = store.lock().unwrap();
                    topics
                        .get(topic)
                        .into_iter()
                        .flatten()
                        .map(|m| {
                            serde_json::json!({ "event": "message", "message": m }).to_string()
                                + "\n"
                        })
                        .collect()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            });
        }
    });
    (url, topics)
}

struct Loopback;

impl Tunnel for Loopback {
    fn alive(&mut self) -> bool {
        true
    }
}

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

fn start(
    ntfy: &str,
    key: &str,
    enc_key: [u8; 32],
    user_id: u64,
    can_host: bool,
) -> (Client, Receiver<Event>) {
    let hosting = can_host.then(|| {
        Hosting(Arc::new(|port: u16| {
            Ok((
                format!("http://127.0.0.1:{port}"),
                Box::new(Loopback) as Box<dyn Tunnel>,
            ))
        }))
    });
    Client::start(Config {
        kind: RelayKind::Hosted,
        relay_url: ntfy.to_owned(),
        room: auth::room_id(key),
        key: key.to_owned(),
        enc_key,
        user_id,
        hosting,
    })
}

/// Waits for a route event, then for a snapshot in which `other` has
/// `selection`; returns the route.
fn route_and_sees(events: &Receiver<Event>, other: u64, selection: &str) -> (Option<Route>, bool) {
    let end = Instant::now() + Duration::from_secs(15);
    let mut route = None;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(Event::Route(r)) => route = Some(r),
            Ok(Event::Snapshot(snapshot)) => {
                if snapshot
                    .users
                    .iter()
                    .any(|u| u.user_id == other && u.selection == [selection])
                {
                    return (route, true);
                }
            }
            Ok(Event::Rejected(why)) => panic!("refused: {why}"),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    (route, false)
}

#[test]
fn the_first_hosts_the_second_joins_and_the_host_says_goodbye() {
    let (ntfy, topics) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let room = auth::room_id(&key);

    let (ana, ana_events) = start(&ntfy, &key, enc, 1, true);
    ana.publish(state(1, "Workspace.Baseplate"));
    // Ana hosts before Ben looks.
    let hosting = loop {
        match ana_events
            .recv_timeout(Duration::from_secs(10))
            .expect("ana routes")
        {
            Event::Route(Route::Hosting(url)) => break url,
            Event::Rejected(why) => panic!("{why}"),
            _ => {}
        }
    };

    // Ben may not host: he must find Ana's announcement.
    let (ben, ben_events) = start(&ntfy, &key, enc, 2, false);
    ben.publish(state(2, "Workspace.SpawnLocation"));
    let (ben_route, ben_sees) = route_and_sees(&ben_events, 1, "Workspace.Baseplate");
    assert_eq!(ben_route, Some(Route::Joined(hosting.clone())));
    assert!(ben_sees);
    let (_, ana_sees) = route_and_sees(&ana_events, 2, "Workspace.SpawnLocation");
    assert!(ana_sees);

    drop(ana);
    let topic = announce::topic(&room);
    let end = Instant::now() + Duration::from_secs(10);
    let said_goodbye = loop {
        let messages = topics
            .lock()
            .unwrap()
            .get(&topic)
            .cloned()
            .unwrap_or_default();
        let latest = announce::newest(
            &key,
            &room,
            rbx_presence::now_ms(),
            messages.iter().map(String::as_str),
        );
        if messages.len() >= 2 && latest.is_none() {
            break true;
        }
        if Instant::now() > end {
            break false;
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert!(said_goodbye, "the host left without a goodbye");
}

#[test]
fn nobody_hosting_and_no_way_to_host_is_waiting() {
    let (ntfy, _topics) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let (_ben, events) = start(&ntfy, &key, crypt::enc_key(&secret, 4567), 2, false);
    let waiting = loop {
        match events.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Route(Route::Waiting)) => break true,
            Ok(Event::Route(other)) => panic!("{other:?}"),
            Ok(_) => {}
            Err(_) => break false,
        }
    };
    assert!(waiting);
}

/// The plugin's own `Relay.luau` in a hosted room: with nobody hosting it
/// waits; once an rbx-native user hosts, it finds the announcement and the
/// two see each other.
#[test]
fn the_plugin_waits_then_finds_the_rbx_native_host() {
    let (ntfy, _topics) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);

    let lua = common::new_lua();
    common::studio_stubs(&lua);
    let setup = format!(
        r##"
        local Auth = script.Parent.Auth
        local Crypto = script.Parent.Crypto
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{secret}", 4567)
        relay = Relay.new({{ kind = "hosted", url = "{ntfy}", key = key, encKey = Crypto.encKey("{secret}", 4567), room = Auth.roomId(key), userId = 7 }})
        local ok, err = relay:publish({{
            v = 2, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
            selection = {{ "Workspace.Baseplate" }}, edits = {{}}, seen = 0,
        }})
        return ok, err, relay:waiting()
        "##
    );
    let (ok, err, waiting): (bool, Option<String>, bool) = lua.load(setup).eval().unwrap();
    assert!(!ok && waiting, "{err:?}");
    let err = err.unwrap_or_default();
    assert!(err.contains("Waiting"), "{err}");

    let (host, host_events) = start(&ntfy, &key, crypt::enc_key(&secret, 4567), 8, true);
    host.publish(state(8, "Workspace.SpawnLocation"));
    loop {
        match host_events
            .recv_timeout(Duration::from_secs(10))
            .expect("hosts")
        {
            Event::Route(Route::Hosting(_)) => break,
            Event::Rejected(why) => panic!("{why}"),
            _ => {}
        }
    }

    let found: bool = lua
        .load(
            r##"
            relay.lastResolve = -math.huge -- skip the 15 s wait between looks
            local ok, err = relay:publish({
                v = 2, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
                selection = { "Workspace.Baseplate" }, edits = {}, seen = 0,
            })
            assert(ok, err)
            local cursor = 0
            for _ = 1, 10 do
                local snapshot, pollErr = relay:poll(cursor)
                assert(snapshot, pollErr)
                cursor = snapshot.cursor
                for _, user in snapshot.users do
                    if user.userId == 8 and user.selection[1] == "Workspace.SpawnLocation" then
                        return not relay:waiting()
                    end
                end
            end
            return false
            "##,
        )
        .eval()
        .unwrap();
    assert!(found);
    let (_, host_sees) = route_and_sees(&host_events, 7, "Workspace.Baseplate");
    assert!(host_sees);
}
