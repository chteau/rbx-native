//! Two editors meeting in a real relay: rbx-native's own host
//! (`rbx_presence::host`), started in-process, so the Rust port of the
//! Worker is held to the same behaviour. Set `RBX_PRESENCE_RELAY` (e.g.
//! `http://127.0.0.1:8787` with `cd relay && npx wrangler dev`) to run the
//! same tests against the Cloudflare Worker instead.

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

fn join(relay: &str, key: &str, user_id: u64) -> (Client, std::sync::mpsc::Receiver<Event>) {
    Client::start(Config {
        kind: rbx_presence::relay::RelayKind::Worker,
        relay_url: relay.to_owned(),
        room: auth::room_id(key),
        key: key.to_owned(),
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
    let key = auth::derive_key(&secret::generate(), 4567);
    let (relay, _host) = relay_for(&key);

    let (ana, ana_events) = join(&relay, &key, 1);
    let (ben, ben_events) = join(&relay, &key, 2);
    ana.publish(state(1, "Workspace.Baseplate"));
    ben.publish(state(2, "Workspace.SpawnLocation"));
    assert!(sees(&ana_events, 2, "Workspace.SpawnLocation"));
    assert!(sees(&ben_events, 1, "Workspace.Baseplate"));

    // Someone who knows the room id but not the key: their own key does
    // not hash to that room, so the relay turns them away.
    let (_eve, eve_events) = Client::start(Config {
        kind: rbx_presence::relay::RelayKind::Worker,
        relay_url: relay.clone(),
        room: auth::room_id(&key),
        key: auth::derive_key(&secret::generate(), 4567),
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

/// The plugin's own `Relay.luau`, over a stand-in for `HttpService`, meets
/// this crate's client in the same room.
#[test]
fn the_plugin_and_the_editor_see_each_other() {
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let (relay, _host) = relay_for(&key);

    let lua = common::new_lua();
    common::studio::stubs(&lua);
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
