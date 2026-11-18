//! Quitting the app makes a host leave its room before the process ends.
//! Its own test binary: `leave_all` stops every worker in the process.

// Only the hosted helpers are used here.
#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::hosted::{fake_ntfy, start, state};
use rbx_presence::client::{leave_all, Event, Route};
use rbx_presence::{announce, auth, crypt, secret};

#[test]
fn leave_all_has_a_live_host_say_goodbye_before_it_returns() {
    let (ntfy, _topics) = fake_ntfy();
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let room = auth::room_id(&key);
    let (host, events) = start(&ntfy, &key, crypt::enc_key(&secret, 4567), 8, true);
    host.publish(state(8, "Workspace.Baseplate"));
    let url = loop {
        match events.recv_timeout(Duration::from_secs(10)).expect("hosts") {
            Event::Route(Route::Hosting(url)) => break url,
            Event::Rejected(why) => panic!("{why}"),
            _ => {}
        }
    };
    assert_eq!(
        announce::find(&ntfy, &key, &room).unwrap().map(|a| a.url),
        Some(url.clone())
    );

    // The editor's own cap: quitting waits no longer than this.
    assert!(
        leave_all(Duration::from_millis(300)),
        "the goodbye took too long"
    );
    assert_eq!(announce::find(&ntfy, &key, &room), Ok(None), "no goodbye");
    let addr = url.trim_start_matches("http://");
    assert!(
        std::net::TcpStream::connect(addr).is_err(),
        "the relay still runs"
    );
    drop(host);
}
