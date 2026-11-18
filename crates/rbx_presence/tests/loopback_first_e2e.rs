//! A host is in its own room on the loopback port before its tunnel is up:
//! Studio on the same computer sees this editor without waiting on the
//! tunnel or the announcement.

// Only the hosted helpers are used here.
#[allow(dead_code)]
mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::hosted::{fake_ntfy, fake_ntfy_limited, start, start_with, state};
use rbx_presence::client::{Client, Config, Event, Hosting, Route, Tunnel};
use rbx_presence::host::Host;
use rbx_presence::relay::RelayKind;
use rbx_presence::{announce, auth, crypt, secret};

struct Held;

impl Tunnel for Held {
    fn alive(&mut self) -> bool {
        true
    }
}

#[test]
fn the_host_is_in_its_loopback_room_while_the_tunnel_starts() {
    let (ntfy, _) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let room = auth::room_id(&key);

    // A tunnel that takes as long as the test likes to come up.
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let hosting = Hosting(Arc::new(move |port: u16| {
        let _ = held.lock().unwrap().recv();
        Ok((
            format!("http://127.0.0.1:{port}"),
            Box::new(Held) as Box<dyn Tunnel>,
        ))
    }));
    let (ana, ana_events) = start_with(&ntfy, &key, enc, 1, Some(hosting));
    ana.publish(state(1, "Workspace.Baseplate"));

    let (_studio, studio_events) = studio_on_loopback(&key, enc);
    assert!(
        sees(&studio_events, 1),
        "Studio sees the host before it is public"
    );
    assert!(
        announce::find(&ntfy, &key, &room).unwrap().is_none(),
        "nothing announced while the tunnel starts"
    );

    release.send(()).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let public = loop {
        let left = end
            .checked_duration_since(Instant::now())
            .expect("ana goes public");
        match ana_events.recv_timeout(left) {
            Ok(Event::Route(Route::Hosting(url))) => break url,
            Ok(_) => {}
            Err(_) => panic!("ana goes public"),
        }
    };
    let found = announce::find(&ntfy, &key, &room)
        .unwrap()
        .expect("announced");
    assert_eq!(found.url, public);
}

/// RbxNative killed: no goodbye, so its tunnel stays the newest announced.
/// Studio on the same computer knows that announcement for one from this
/// computer (the loopback port told it the id), so it never asks for the dead tunnel,
/// which in a store install is a permission prompt, and it's back on the
/// loopback port the moment a new host is. A long-poll still waiting its
/// turn on a host the room has since left gives up rather than going there.
#[test]
fn studio_never_follows_its_killed_host_to_the_tunnel() {
    let (ntfy, _) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let room = auth::room_id(&key);
    let port = announce::loopback_port(&room);
    let (tunnel, asked) = dead_tunnel();
    let install = announce::host_id("this-computer");
    let host = Host::start_on(&key, port, &install).expect("port free");
    announce::publish(&ntfy, &key, &room, &tunnel, host.id()).unwrap();

    let lua = plugin(&ntfy, &secret, true);
    let on_loopback = |lua: &mlua::Lua| -> (bool, Option<String>, Option<String>) {
        lua.load("local ok, err = publish() return ok and relay.base == relay.loopback, err, relay.localHost")
            .eval()
            .unwrap()
    };
    let (ok, err, id) = on_loopback(&lua);
    assert!(ok, "{err:?}");
    assert_eq!(id.as_deref(), Some(host.id()));

    drop(host);
    wait_closed(port);
    let (ok, err, base): (bool, String, Option<String>) = lua
        .load(
            r#"
            publish() -- finds the host gone
            relay.lastResolve = -math.huge -- skip the wait between looks
            local ok, err = publish()
            return ok, err, relay.base
            "#,
        )
        .eval()
        .unwrap();
    assert!(!ok && err.contains("Waiting") && base.is_none(), "{err}");
    assert_eq!(asked.load(Ordering::SeqCst), 0, "asked for the dead tunnel");

    let next = Host::start_on(&key, port, &install).expect("port free again");
    lua.load("relay.lastLoopback = -math.huge").exec().unwrap();
    let (ok, err, id) = on_loopback(&lua);
    assert!(ok, "{err:?}");
    assert_eq!(id.as_deref(), Some(next.id()));

    let polled: bool = lua
        .load(format!(
            r#"
            relay.base = "{tunnel}"
            task = {{ wait = function() relay.base = relay.loopback end }}
            local snapshot, err = relay:poll(0)
            assert(snapshot, err)
            return relay.base == relay.loopback
            "#
        ))
        .eval()
        .unwrap();
    assert!(polled);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "went to a host it had left"
    );
}

/// ntfy.sh answering 429, the address over its request budget (Studio's
/// polling on the same address spends it too): the host still serves the
/// loopback port, gets its announcement out once ntfy takes posts again,
/// and spaces its tries out instead of hammering on.
#[test]
fn the_host_rides_out_ntfy_rate_limits() {
    let (refuse, seen) = (Arc::new(AtomicUsize::new(6)), Arc::default());
    let (ntfy, _) = fake_ntfy_limited(refuse.clone(), Arc::clone(&seen));
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let (ana, ana_events) = start(&ntfy, &key, enc, 1, true);
    ana.publish(state(1, "Workspace.Baseplate"));
    let (_studio, studio_events) = studio_on_loopback(&key, enc);
    assert!(
        sees(&studio_events, 1),
        "Studio here sees the host meanwhile"
    );

    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let left = end.checked_duration_since(Instant::now());
        match ana_events.recv_timeout(left.expect("ana goes public")) {
            Ok(Event::Route(Route::Hosting(_))) => break,
            Ok(_) => {}
            Err(_) => panic!("ana goes public"),
        }
    }
    assert_eq!(refuse.load(Ordering::SeqCst), 0);
    let asked = seen.load(Ordering::SeqCst);
    assert!(asked <= 10, "{asked} requests to ntfy");
    assert!(announce::find(&ntfy, &key, &auth::room_id(&key))
        .unwrap()
        .is_some());
}

/// The plugin after a 429 from ntfy: no ntfy request at all until the hold
/// is up, and none while the host answers on the loopback port.
#[test]
fn the_plugin_holds_off_ntfy() {
    let seen = Arc::new(AtomicUsize::new(0));
    let (ntfy, _) = fake_ntfy_limited(Arc::new(AtomicUsize::new(1)), Arc::clone(&seen));
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let port = announce::loopback_port(&auth::room_id(&key));
    let lua = plugin(&ntfy, &secret, false);
    let looks = |lua: &mlua::Lua| -> String {
        lua.load("relay.lastResolve = -math.huge; local _, err = publish(); return err or 'ok'")
            .eval()
            .unwrap()
    };
    assert!(looks(&lua).contains("Waiting"));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    for _ in 0..5 {
        assert!(looks(&lua).contains("Waiting"));
    }
    assert_eq!(seen.load(Ordering::SeqCst), 1, "asked again while held off");
    let (held, posts): (f64, f64) = lua
        .load("return relay.ntfyUntil.GET - os.clock(), relay.ntfyUntil.POST - os.clock()")
        .eval()
        .unwrap();
    assert!(held > 25.0 && posts > 25.0, "held off for {held} s");

    // A refused post (ntfy.sh's daily message cap) holds back posts only.
    let posted = Arc::new(AtomicUsize::new(0));
    let (other, _) = fake_ntfy_limited(Arc::new(AtomicUsize::new(1)), Arc::clone(&posted));
    let beacon = plugin(&other, &secret, false);
    let reads_free: bool = beacon
        .load("relay:keyConflict() return relay.ntfyUntil.POST > os.clock() and relay.ntfyUntil.GET < os.clock()")
        .eval()
        .unwrap();
    assert!(reads_free);
    assert_eq!(posted.load(Ordering::SeqCst), 2, "the beacon read went out");

    let _host = Host::start_on(&key, port, "h").expect("port free");
    let lua = plugin(&ntfy, &secret, true);
    for _ in 0..5 {
        assert_eq!(looks(&lua), "ok");
    }
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "asked ntfy while on loopback"
    );
    let every: f64 = lua.load("return relay:_resolveEvery()").eval().unwrap();
    assert_eq!(every, 30.0);
}

/// The plugin's relay for the room of `secret` on place 4567, with a
/// `publish()` sending Ana's state.
fn plugin(ntfy: &str, secret: &str, loopback_ok: bool) -> mlua::Lua {
    let lua = common::new_lua();
    common::studio_stubs(&lua);
    lua.load(format!(
        r##"
        local Auth = script.Parent.Auth
        local Crypto = script.Parent.Crypto
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{secret}", 4567)
        relay = Relay.new({{ kind = "hosted", url = "{ntfy}", key = key, encKey = Crypto.encKey("{secret}", 4567), room = Auth.roomId(key), userId = 7, placeId = 4567, loopbackOk = {loopback_ok} }})
        function publish()
            return relay:publish({{
                v = 2, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
                selection = {{ "Workspace.Baseplate" }}, edits = {{}}, seen = 0,
            }})
        end
        "##
    ))
    .exec()
    .unwrap();
    lua
}

/// Studio on this computer, talking straight to the room's loopback port.
fn studio_on_loopback(key: &str, enc: [u8; 32]) -> (Client, Receiver<Event>) {
    let room = auth::room_id(key);
    let (studio, events) = Client::start(Config {
        kind: RelayKind::Worker,
        relay_url: format!("http://127.0.0.1:{}", announce::loopback_port(&room)),
        room,
        key: key.to_owned(),
        enc_key: enc,
        user_id: 2,
        place_id: 4567,
        beacon_from: "studio".into(),
        hosting: None,
    });
    studio.publish(state(2, "Workspace.SpawnLocation"));
    (studio, events)
}

/// What a killed quick tunnel's address answers (Cloudflare's 530), and how
/// many times it was asked.
fn dead_tunnel() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            count.fetch_add(1, Ordering::SeqCst);
            let mut head = [0; 4096];
            let _ = stream.read(&mut head);
            let _ = stream
                .write_all(b"HTTP/1.1 530 \r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        }
    });
    (url, asked)
}

/// Until nothing listens on `port` (a dropped host closes it on its own thread).
fn wait_closed(port: u16) {
    let end = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(Instant::now() < end, "the host let go of its port");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether a snapshot with `user` arrives within a few seconds.
fn sees(events: &Receiver<Event>, user: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(8);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(Event::Snapshot(s)) if s.users.iter().any(|u| u.user_id == user) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}
