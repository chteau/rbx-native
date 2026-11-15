//! A hosted room end to end, on this machine: a stand-in ntfy server, one
//! rbx-native client allowed to host (its "tunnel" is the loopback port
//! itself) and one that only joins. The first hosts and announces, the
//! second finds the announcement and joins, they see each other, and the
//! host says goodbye when it stops.

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use common::hosted::{fake_ntfy, start, state};
use rbx_presence::client::{Event, Route};
use rbx_presence::{announce, auth, crypt, secret};

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
        relay = Relay.new({{ kind = "hosted", url = "{ntfy}", key = key, encKey = Crypto.encKey("{secret}", 4567), room = Auth.roomId(key), userId = 7, placeId = 4567 }})
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

/// The host quits without a goodbye (its tunnel answers 530): the plugin
/// stops naming it once the grace runs out and waits for someone again,
/// then joins the next host by itself. When that one leaves with a goodbye
/// the plugin goes back to waiting right away.
#[test]
fn the_plugin_drops_a_vanished_host_and_joins_the_next() {
    let (ntfy, _topics) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let room = auth::room_id(&key);
    // What a dead quick tunnel's address answers: Cloudflare's 530 page.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut head = [0; 4096];
            let _ = stream.read(&mut head);
            let page = "<html>origin unreachable</html>";
            let _ = write!(
                stream,
                "HTTP/1.1 530 \r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    announce::publish(&ntfy, &key, &room, &dead).unwrap();

    let lua = common::new_lua();
    common::studio_stubs(&lua);
    let setup = format!(
        r##"
        local Auth = script.Parent.Auth
        local Crypto = script.Parent.Crypto
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{secret}", 4567)
        relay = Relay.new({{ kind = "hosted", url = "{ntfy}", key = key, encKey = Crypto.encKey("{secret}", 4567), room = Auth.roomId(key), userId = 7, placeId = 4567 }})
        function publish()
            return relay:publish({{
                v = 2, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
                selection = {{ "Workspace.Baseplate" }}, edits = {{}}, seen = 0,
            }})
        end
        local ok, err = publish()
        return ok, err, relay:waiting()
        "##
    );
    let (ok, err, waiting): (bool, String, bool) = lua.load(setup).eval().unwrap();
    assert!(
        !ok && !waiting && err.contains("Can't reach") && err.contains("530"),
        "{err}"
    );

    // Past the grace, the next look finds the same dead address: waiting.
    let (ok, err, waiting): (bool, String, bool) = lua
        .load(
            r#"
            relay.goneSince -= 60
            relay.lastResolve = -math.huge
            local ok, err = publish()
            return ok, err, relay:waiting()
            "#,
        )
        .eval()
        .unwrap();
    assert!(
        !ok && waiting && err.contains("Waiting for someone"),
        "{err}"
    );

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
            r#"
            relay.lastResolve = -math.huge
            local ok, err = publish()
            assert(ok, err)
            local cursor = 0
            for _ = 1, 10 do
                local snapshot, pollErr = relay:poll(cursor)
                assert(snapshot, pollErr)
                cursor = snapshot.cursor
                for _, user in snapshot.users do
                    if user.userId == 8 then
                        return not relay:waiting() and relay.goneSince == nil
                    end
                end
            end
            return false
            "#,
        )
        .eval()
        .unwrap();
    assert!(found);

    drop(host);
    let end = Instant::now() + Duration::from_secs(10);
    while announce::find(&ntfy, &key, &room) != Ok(None) {
        assert!(Instant::now() < end, "the host left without a goodbye");
        thread::sleep(Duration::from_millis(100));
    }
    let waiting: bool = lua
        .load(
            r#"
            for _ = 1, 2 do
                publish()
            end
            return relay:waiting()
            "#,
        )
        .eval()
        .unwrap();
    assert!(
        waiting,
        "a host that said goodbye leaves nothing to wait out"
    );
}

/// Studio and rbx-native on one place with different keys (each made its
/// own before the place was saved) see each other's beacon and say so,
/// instead of each waiting for the other forever.
#[test]
fn different_keys_on_one_place_are_reported_on_both_sides() {
    let (ntfy, _topics) = fake_ntfy();
    let studio_secret = secret::generate();
    let lua = common::new_lua();
    common::studio_stubs(&lua);
    let setup = format!(
        r##"
        local Auth = script.Parent.Auth
        local Crypto = script.Parent.Crypto
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{studio_secret}", 4567)
        relay = Relay.new({{ kind = "hosted", url = "{ntfy}", key = key, encKey = Crypto.encKey("{studio_secret}", 4567), room = Auth.roomId(key), userId = 7, placeId = 4567 }})
        return relay:keyConflict()
        "##
    );
    let alone: Option<String> = lua.load(setup).eval().unwrap();
    assert_eq!(alone, None);

    let native_secret = secret::generate();
    let key = auth::derive_key(&native_secret, 4567);
    let (_host, events) = start(&ntfy, &key, crypt::enc_key(&native_secret, 4567), 7, true);
    let seen = loop {
        match events
            .recv_timeout(Duration::from_secs(10))
            .expect("reports")
        {
            Event::KeyConflict(found) => break found,
            Event::Rejected(why) => panic!("{why}"),
            _ => {}
        }
    };
    assert_eq!(seen.as_deref(), Some("studio"));

    let found: Option<String> = lua
        .load("relay.lastBeaconRead = -math.huge; return relay:keyConflict()")
        .eval()
        .unwrap();
    assert_eq!(found.as_deref(), Some("rbx-native"));
}
