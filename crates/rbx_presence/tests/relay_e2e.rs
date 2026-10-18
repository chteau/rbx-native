//! Two editors meeting in a real relay: rbx-native's own host
//! (`rbx_presence::host`), started in-process, so the Rust port of the
//! Worker is held to the same behaviour. Set `RBX_PRESENCE_RELAY` (e.g.
//! `http://127.0.0.1:8787` with `cd relay && npx wrangler dev`) to run the
//! same tests against the Cloudflare Worker instead.

mod common;

use std::time::{Duration, Instant};

use rbx_presence::client::{Client, Config, Event};
use rbx_presence::schema::{Client as Editor, UserState, SCHEMA_VERSION};
use rbx_presence::schema::{Sealed, SealedSnapshot};
use rbx_presence::{auth, crypt, seal, secret};

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

/// The relay under test for `key`'s room, and what keeps it running.
fn relay_for(key: &str) -> (String, Option<rbx_presence::host::Host>) {
    match std::env::var("RBX_PRESENCE_RELAY") {
        Ok(url) => (url, None),
        Err(_) => {
            let host = rbx_presence::host::Host::start(key).expect("host starts");
            (host.local_url(), Some(host))
        }
    }
}

fn join(
    relay: &str,
    key: &str,
    enc_key: [u8; 32],
    user_id: u64,
) -> (Client, std::sync::mpsc::Receiver<Event>) {
    Client::start(Config {
        kind: rbx_presence::relay::RelayKind::Worker,
        relay_url: relay.to_owned(),
        room: auth::room_id(key),
        key: key.to_owned(),
        enc_key,
        user_id,
        hosting: None,
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
fn two_editors_see_each_other_and_a_stranger_is_refused() {
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let (relay, _host) = relay_for(&key);

    let (ana, ana_events) = join(&relay, &key, enc, 1);
    let (ben, ben_events) = join(&relay, &key, enc, 2);
    ana.publish(state(1, "Workspace.Baseplate"));
    ben.publish(state(2, "Workspace.SpawnLocation"));
    assert!(sees(&ana_events, 2, "Workspace.SpawnLocation"));
    assert!(sees(&ben_events, 1, "Workspace.Baseplate"));

    // Someone who knows the room id but not the key: their own key does
    // not hash to that room, so the relay turns them away.
    let eve_secret = secret::generate();
    let (_eve, eve_events) = Client::start(Config {
        kind: rbx_presence::relay::RelayKind::Worker,
        relay_url: relay.clone(),
        room: auth::room_id(&key),
        key: auth::derive_key(&eve_secret, 4567),
        enc_key: crypt::enc_key(&eve_secret, 4567),
        user_id: 3,
        hosting: None,
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

#[test]
fn the_relay_only_ever_holds_ciphertext() {
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let room = auth::room_id(&key);
    let (relay, _host) = relay_for(&key);

    let mut mine = state(1, "Workspace.SecretVault");
    mine.name = "Zebra Quill".to_owned();
    let sealed = seal::seal_state(&enc, &room, &mine);
    let body = serde_json::to_string(&sealed).unwrap();
    let path = format!("/v1/rooms/{room}/state");
    let (status, reply) = common::signed_request(&relay, &key, 1, "POST", &path, &body);
    assert_eq!(status, 200, "{reply}");

    let path = format!("/v1/rooms/{room}/poll?cursor=0");
    let (status, reply) = common::signed_request(&relay, &key, 1, "GET", &path, "");
    assert_eq!(status, 200, "{reply}");
    assert!(!reply.contains("Zebra Quill"), "{reply}");
    assert!(!reply.contains("SecretVault"), "{reply}");
    let snapshot: SealedSnapshot = serde_json::from_str(&reply).expect("sealed snapshot");
    let held: &Sealed = snapshot.users.first().expect("the sealed state is held");
    assert!(!held.ct.is_empty());
    let opened = seal::open_state(&enc, &room, held).expect("opens with the encryption key");
    assert_eq!(opened.name, "Zebra Quill");
    assert_eq!(opened.selection, ["Workspace.SecretVault"]);
}

#[test]
fn a_cleartext_v1_state_is_refused_with_version() {
    let key = auth::derive_key(&secret::generate(), 4567);
    let room = auth::room_id(&key);
    let (relay, _host) = relay_for(&key);

    let mut old = state(1, "Workspace.Baseplate");
    old.v = 1;
    let body = serde_json::to_string(&old).unwrap();
    let path = format!("/v1/rooms/{room}/state");
    let (status, reply) = common::signed_request(&relay, &key, 1, "POST", &path, &body);
    assert_eq!(status, 400, "{reply}");
    assert!(reply.contains("version"), "{reply}");
}

#[test]
fn a_state_sealed_under_another_key_never_reaches_a_snapshot() {
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let enc = crypt::enc_key(&secret, 4567);
    let (relay, _host) = relay_for(&key);

    let (ana, ana_events) = join(&relay, &key, enc, 1);
    ana.publish(state(1, "Workspace.Baseplate"));
    let (eve, _eve_events) = join(&relay, &key, crypt::enc_key("another secret", 4567), 3);
    eve.publish(state(3, "Workspace.Intruder"));
    std::thread::sleep(Duration::from_millis(500));
    let (ben, _ben_events) = join(&relay, &key, enc, 2);
    ben.publish(state(2, "Workspace.SpawnLocation"));

    let end = Instant::now() + Duration::from_secs(15);
    let mut saw_ben = false;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match ana_events.recv_timeout(left) {
            Ok(Event::Snapshot(snapshot)) => {
                assert!(snapshot.users.iter().all(|u| u.user_id != 3));
                if snapshot.users.iter().any(|u| u.user_id == 2) {
                    saw_ben = true;
                    break;
                }
            }
            Ok(Event::Rejected(why)) => panic!("relay refused: {why}"),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(saw_ben);
}

/// The plugin's own `Relay.luau`, over a stand-in for `HttpService`, meets
/// this crate's client in the same room.
#[test]
fn the_plugin_and_the_editor_see_each_other() {
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let (relay, _host) = relay_for(&key);

    let lua = common::new_lua();
    common::studio_stubs(&lua);
    let (editor, editor_events) = join(&relay, &key, crypt::enc_key(&secret, 4567), 8);
    editor.publish(state(8, "Workspace.SpawnLocation"));

    let chunk = format!(
        r##"
        local Auth = script.Parent.Auth
        local Crypto = script.Parent.Crypto
        local Relay = script.Parent.Relay
        local key = Auth.deriveKey("{secret}", 4567)
        local relay = Relay.new({{ url = "{relay}", key = key, encKey = Crypto.encKey("{secret}", 4567), room = Auth.roomId(key), userId = 7 }})
        local ok, err = relay:publish({{
            v = 2, userId = 7, name = "Ana", color = "#e8590c", client = "studio",
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
