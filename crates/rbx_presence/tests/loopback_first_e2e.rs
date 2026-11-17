//! A host is in its own room on the loopback port before its tunnel is up:
//! Studio on the same computer sees this editor without waiting on the
//! tunnel or the announcement.

// Only the hosted helpers are used here.
#[allow(dead_code)]
mod common;

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::hosted::{fake_ntfy, start_with, state};
use rbx_presence::client::{Client, Config, Event, Hosting, Route, Tunnel};
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

    // Studio on this computer: straight to the room's loopback port.
    let (studio, studio_events) = Client::start(Config {
        kind: RelayKind::Worker,
        relay_url: format!("http://127.0.0.1:{}", announce::loopback_port(&room)),
        room: room.clone(),
        key: key.clone(),
        enc_key: enc,
        user_id: 2,
        place_id: 4567,
        beacon_from: "studio".into(),
        hosting: None,
    });
    studio.publish(state(2, "Workspace.SpawnLocation"));
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
