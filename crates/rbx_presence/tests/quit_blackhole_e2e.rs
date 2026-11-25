//! Quitting with ntfy out of reach (its address taking connections but
//! never answering) doesn't wait on it. Its own test binary: `leave_all`
//! stops every worker in the process.

// Only the hosted helpers are used here.
#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use common::hosted::{start, state};
use rbx_presence::client::leave_all;
use rbx_presence::{auth, crypt, secret};

#[test]
fn a_hanging_ntfy_does_not_hold_up_quitting() {
    // Never accepted: connections sit in the backlog and no byte comes back.
    let blackhole = rbx_presence::announce::bind_outside_loopback_range().unwrap();
    let ntfy = format!("http://{}", blackhole.local_addr().unwrap());
    let secret = secret::generate();
    let key = auth::derive_key(&secret, 4567);
    let (host, _events) = start(&ntfy, &key, crypt::enc_key(&secret, 4567), 8, true);
    host.publish(state(8, "Workspace.Baseplate"));
    // Long enough for the worker to be stuck in a request to it.
    std::thread::sleep(Duration::from_secs(1));

    let quit = Instant::now();
    let all_left = leave_all(Duration::from_millis(300));
    let took = quit.elapsed();
    assert!(!all_left, "the worker wasn't waiting on ntfy at all");
    assert!(took < Duration::from_millis(500), "quitting took {took:?}");
    drop(host);
}
