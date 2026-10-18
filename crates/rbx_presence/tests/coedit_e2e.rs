//! Co-editing channels meeting in rbx-native's own relay host, in-process.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use rbx_presence::coedit::channel::{Channel, Config, Event};
use rbx_presence::coedit::{Batch, Clock, Op, Target};
use rbx_presence::schema::Client as Editor;
use rbx_presence::{auth, crypt};

const KEY: &str = "coedit-e2e-key";

fn config(host: &rbx_presence::host::Host, enc_key: [u8; 32], user_id: u64) -> Config {
    Config {
        relay_url: host.local_url(),
        room: auth::room_id(KEY),
        key: KEY.to_owned(),
        enc_key,
        user_id,
        client: Editor::RbxNative,
    }
}

fn del_batch(user_id: u64, n: u64) -> Batch {
    Batch {
        a: format!("{user_id}:rbx-native"),
        ops: vec![Op::Del {
            at: Target {
                id: format!("{n:032x}"),
                p: format!("Workspace.P{n}"),
            },
            c: Clock(n, format!("{user_id}:rbx-native")),
        }],
    }
}

fn batches(events: &Receiver<Event>, count: usize) -> Vec<(u64, Batch)> {
    let end = Instant::now() + Duration::from_secs(15);
    let mut got = Vec::new();
    while got.len() < count {
        let Some(left) = end.checked_duration_since(Instant::now()) else {
            panic!("only {} of {count} batches arrived", got.len());
        };
        match events.recv_timeout(left) {
            Ok(Event::Batch { seq, batch, .. }) => got.push((seq, batch)),
            Ok(Event::Gap) => panic!("unexpected gap"),
            Ok(Event::Error(why)) => panic!("channel error: {why}"),
            Err(_) => panic!("only {} of {count} batches arrived", got.len()),
        }
    }
    got
}

#[test]
fn two_channels_see_the_same_order() {
    let host = rbx_presence::host::Host::start(KEY).expect("host starts");
    let enc = crypt::enc_key("secret", 1);
    let (a, a_events) = Channel::start(config(&host, enc, 1));
    let (b, b_events) = Channel::start(config(&host, enc, 2));
    a.send(del_batch(1, 1));
    b.send(del_batch(2, 2));
    let from_a = batches(&a_events, 2);
    let from_b = batches(&b_events, 2);
    assert_eq!(from_a, from_b);
    let seqs: Vec<u64> = from_a.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(seqs, vec![1, 2]);
}

#[test]
fn large_source_batch_arrives_whole() {
    let host = rbx_presence::host::Host::start(KEY).expect("host starts");
    let enc = crypt::enc_key("secret", 1);
    let (a, a_events) = Channel::start(config(&host, enc, 1));
    let text: String = (0..60 * 1024)
        .map(|i| (b'a' + (i % 26) as u8) as char)
        .collect();
    let batch = Batch {
        a: "1:rbx-native".into(),
        ops: vec![Op::Src {
            at: Target {
                id: "x".repeat(32),
                p: "ServerScriptService.Big".into(),
            },
            base: "0".into(),
            text,
            c: Clock(1, "1:rbx-native".into()),
        }],
    };
    a.send(batch.clone());
    let got = batches(&a_events, 1);
    assert_eq!(got[0].1, batch);
}

#[test]
fn other_enc_key_receives_nothing() {
    let host = rbx_presence::host::Host::start(KEY).expect("host starts");
    let (a, a_events) = Channel::start(config(&host, crypt::enc_key("secret", 1), 1));
    let (_b, b_events) = Channel::start(config(&host, crypt::enc_key("other", 1), 2));
    a.send(del_batch(1, 1));
    batches(&a_events, 1);
    let end = Instant::now() + Duration::from_millis(1500);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Ok(Event::Batch { .. }) = b_events.recv_timeout(left) {
            panic!("a channel with another key opened a batch");
        }
    }
}

#[test]
fn late_channel_replays_the_log() {
    let host = rbx_presence::host::Host::start(KEY).expect("host starts");
    let enc = crypt::enc_key("secret", 1);
    let (a, a_events) = Channel::start(config(&host, enc, 1));
    for n in 1..=3 {
        a.send(del_batch(1, n));
    }
    let first = batches(&a_events, 3);
    let (_late, late_events) = Channel::start(config(&host, enc, 2));
    assert_eq!(batches(&late_events, 3), first);
}
