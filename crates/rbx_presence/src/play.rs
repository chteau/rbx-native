//! Local Run play through the room's relay: collaborators joining a Local
//! Run as players send their input to the session's host, and the host
//! sends each of them their client's frames, as opaque messages addressed
//! from one end to another.
//!
//! Each editor opens one WebSocket, `<relay>/v1/rooms/<room>/play?pid=<id>`,
//! signed like the presence socket, `id` a random 64-bit end for this
//! editor (one user may be in two editors). A message goes out as one or
//! more binary WebSocket messages, `to` (8 bytes, big-endian; 0 for
//! everyone in the room) then a chunk sealed with the place key
//! ([`crate::crypt::play_aad`] binds room, sender and receiver), and comes
//! in with the relay's `from` in place of `to`. The relay forwards them and
//! keeps nothing; it can't read them, nor make one end pass for another.
//! A chunk is at most [`CHUNK_BYTES`]; a big message (an avatar, a place's
//! first frame) is cut and put back together on arrival.
//!
//! Nothing is kept for an end that isn't connected: a dropped connection
//! loses what was in flight, and [`Event::Connected`] after a
//! [`Event::Disconnected`] is the caller's cue to ask for a fresh start.

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::client::connect_route;
use crate::crypt;

/// The most of a message one WebSocket message carries.
pub const CHUNK_BYTES: usize = 192 * 1024;
/// The largest WebSocket message the play route takes: a chunk, its
/// header, the seal and the address.
pub const MESSAGE_BYTES: usize = CHUNK_BYTES + 64;
/// How long a read waits before the outbox is looked at again: the most a
/// message waits to leave.
pub const TICK: Duration = Duration::from_millis(4);
const BACKOFF_START: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// Messages half put together, before the oldest are given up on.
const MOST_PARTIAL: usize = 32;

/// Where and as whom to connect.
#[derive(Debug, Clone)]
pub struct Config {
    /// The relay, as the presence client reaches it.
    pub relay_url: String,
    pub room: String,
    pub key: String,
    pub enc_key: [u8; crypt::KEY_LEN],
    pub user_id: u64,
    /// This editor's end ([`new_pid`]).
    pub pid: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Message {
        from: u64,
        bytes: Vec<u8>,
    },
    /// Lost or refused; it tries again.
    Disconnected(String),
}

/// Bytes on the wire so far, both ways, seals and addresses included.
#[derive(Debug, Default)]
pub struct Stats {
    pub sent: AtomicU64,
    pub received: AtomicU64,
}

/// The handle; dropping it closes the connection.
pub struct Link {
    out: Sender<(u64, Vec<u8>)>,
    stop: Arc<AtomicBool>,
    stats: Arc<Stats>,
}

/// A fresh random end, never 0 (that's everyone).
pub fn new_pid() -> u64 {
    loop {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the OS random source is available");
        let pid = u64::from_be_bytes(bytes);
        if pid != 0 {
            return pid;
        }
    }
}

impl Link {
    pub fn start(config: Config) -> (Link, Receiver<Event>) {
        let (out, out_rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::default());
        let (thread_stop, thread_stats) = (stop.clone(), stats.clone());
        thread::Builder::new()
            .name("presence-play".into())
            .spawn(move || run(config, out_rx, events_tx, thread_stop, thread_stats))
            .expect("a thread for Local Run play");
        (Link { out, stop, stats }, events)
    }

    /// Sends `bytes` to end `to` (0: everyone else in the room). Dropped
    /// while disconnected.
    pub fn send(&self, to: u64, bytes: Vec<u8>) {
        let _ = self.out.send((to, bytes));
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// `bytes` cut into chunks, each `id`, its index and the count first.
fn chunks(id: u32, bytes: &[u8]) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = if bytes.is_empty() {
        vec![&[]]
    } else {
        bytes.chunks(CHUNK_BYTES).collect()
    };
    let count = parts.len() as u16;
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let mut chunk = Vec::with_capacity(part.len() + 8);
            chunk.extend(id.to_be_bytes());
            chunk.extend((i as u16).to_be_bytes());
            chunk.extend(count.to_be_bytes());
            chunk.extend_from_slice(part);
            chunk
        })
        .collect()
}

/// Chunks put back together, per sender and message.
#[derive(Default)]
struct Partial {
    open: HashMap<(u64, u32), Vec<Option<Vec<u8>>>>,
    order: Vec<(u64, u32)>,
}

impl Partial {
    /// The whole message once `chunk` from `from` was its last piece.
    fn add(&mut self, from: u64, chunk: &[u8]) -> Option<Vec<u8>> {
        if chunk.len() < 8 {
            return None;
        }
        let id = u32::from_be_bytes(chunk[..4].try_into().ok()?);
        let index = usize::from(u16::from_be_bytes(chunk[4..6].try_into().ok()?));
        let count = usize::from(u16::from_be_bytes(chunk[6..8].try_into().ok()?));
        let data = &chunk[8..];
        if count == 1 {
            return Some(data.to_vec());
        }
        if index >= count {
            return None;
        }
        let key = (from, id);
        if !self.open.contains_key(&key) {
            if self.order.len() >= MOST_PARTIAL {
                let oldest = self.order.remove(0);
                self.open.remove(&oldest);
            }
            self.order.push(key);
        }
        let parts = self.open.entry(key).or_insert_with(|| vec![None; count]);
        if parts.len() != count {
            return None;
        }
        parts[index] = Some(data.to_vec());
        if parts.iter().any(Option::is_none) {
            return None;
        }
        let parts = self.open.remove(&key)?;
        self.order.retain(|k| *k != key);
        Some(parts.into_iter().flatten().flatten().collect())
    }
}

fn nap(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() && !stop.load(Ordering::SeqCst) {
        let now = left.min(step);
        thread::sleep(now);
        left -= now;
    }
}

fn run(
    config: Config,
    out: Receiver<(u64, Vec<u8>)>,
    events: Sender<Event>,
    stop: Arc<AtomicBool>,
    stats: Arc<Stats>,
) {
    let route = format!("play?pid={:016x}", config.pid);
    let mut delay = BACKOFF_START;
    let mut next_id = 0u32;
    while !stop.load(Ordering::SeqCst) {
        // What waited for a connection is stale by now (input, frames).
        while out.try_recv().is_ok() {}
        let why = match connect_route(
            &config.relay_url,
            &config.room,
            &config.key,
            config.user_id,
            &route,
            TICK,
        ) {
            Ok(mut socket) => {
                delay = BACKOFF_START;
                if events.send(Event::Connected).is_err() {
                    return;
                }
                match session(
                    &config,
                    &mut socket,
                    &out,
                    &events,
                    &stop,
                    &stats,
                    &mut next_id,
                ) {
                    Some(why) => why,
                    None => return,
                }
            }
            Err(e) => e.to_string(),
        };
        if events.send(Event::Disconnected(why)).is_err() {
            return;
        }
        nap(&stop, delay);
        delay = (delay * 2).min(BACKOFF_MAX);
    }
}

/// One connection, until it fails (why) or the link is dropped (`None`).
fn session(
    config: &Config,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    out: &Receiver<(u64, Vec<u8>)>,
    events: &Sender<Event>,
    stop: &AtomicBool,
    stats: &Stats,
    next_id: &mut u32,
) -> Option<String> {
    let mut partial = Partial::default();
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = socket.close(None);
            let _ = socket.flush();
            return None;
        }
        loop {
            let (to, bytes) = match out.try_recv() {
                Ok(next) => next,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return None;
                }
            };
            let aad = crypt::play_aad(&config.room, config.pid, to);
            for chunk in chunks(*next_id, &bytes) {
                let mut message = to.to_be_bytes().to_vec();
                message.extend(crypt::seal(&config.enc_key, &aad, &chunk));
                stats
                    .sent
                    .fetch_add(message.len() as u64, Ordering::Relaxed);
                if let Err(e) = socket.send(Message::binary(message)) {
                    return Some(e.to_string());
                }
            }
            *next_id = next_id.wrapping_add(1);
        }
        match socket.read() {
            Ok(Message::Binary(data)) if data.len() > 8 => {
                stats
                    .received
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
                let from = u64::from_be_bytes(data[..8].try_into().unwrap_or_default());
                let sealed = &data[8..];
                let chunk = [config.pid, 0].into_iter().find_map(|to| {
                    crypt::open(
                        &config.enc_key,
                        &crypt::play_aad(&config.room, from, to),
                        sealed,
                    )
                });
                if let Some(bytes) = chunk.and_then(|c| partial.add(from, &c)) {
                    if events.send(Event::Message { from, bytes }).is_err() {
                        return None;
                    }
                }
            }
            Ok(Message::Close(_)) => return Some("the relay closed the connection".into()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Some(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn config(url: &str, key: &str, secret: &str, pid: u64) -> Config {
        Config {
            relay_url: url.into(),
            room: crate::auth::room_id(key),
            key: key.into(),
            enc_key: crypt::enc_key(secret, 1),
            user_id: 7,
            pid,
        }
    }

    fn next(events: &Receiver<Event>) -> Event {
        events
            .recv_timeout(Duration::from_secs(10))
            .expect("an event in time")
    }

    fn message(events: &Receiver<Event>) -> (u64, Vec<u8>) {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if let Event::Message { from, bytes } = next(events) {
                return (from, bytes);
            }
        }
        panic!("no message");
    }

    /// Two ends through a relay on loopback: addressed and broadcast
    /// messages, a big one in chunks, and an end with another place key
    /// hears nothing it can read.
    #[test]
    fn ends_reach_each_other_through_the_relay_sealed() {
        let key = crate::auth::derive_key("secret", 1);
        let host = crate::host::Host::start(&key).unwrap();
        let url = host.local_url();
        let (a, a_events) = Link::start(config(&url, &key, "secret", 1));
        let (b, b_events) = Link::start(config(&url, &key, "secret", 2));
        // Same room, wrong place key: it can connect but not read.
        let (_c, c_events) = Link::start(config(&url, &key, "other", 3));
        for events in [&a_events, &b_events, &c_events] {
            assert_eq!(next(events), Event::Connected);
        }
        a.send(2, b"to b".to_vec());
        assert_eq!(message(&b_events), (1, b"to b".to_vec()));
        b.send(0, b"to everyone".to_vec());
        assert_eq!(message(&a_events), (2, b"to everyone".to_vec()));
        let big: Vec<u8> = (0..CHUNK_BYTES * 2 + 5).map(|i| i as u8).collect();
        a.send(2, big.clone());
        assert_eq!(message(&b_events), (1, big));
        assert!(
            c_events.recv_timeout(Duration::from_millis(300)).is_err(),
            "another key reads nothing"
        );
        assert!(a.stats().sent.load(Ordering::Relaxed) > (CHUNK_BYTES * 2) as u64);
        assert!(b.stats().received.load(Ordering::Relaxed) > (CHUNK_BYTES * 2) as u64);
    }

    #[test]
    fn a_message_is_cut_and_put_back_together() {
        let bytes: Vec<u8> = (0..CHUNK_BYTES + 10).map(|i| (i % 251) as u8).collect();
        let parts = chunks(9, &bytes);
        assert_eq!(parts.len(), 2);
        let mut partial = Partial::default();
        assert_eq!(partial.add(5, &parts[1]), None);
        assert_eq!(partial.add(5, &parts[0]), Some(bytes));
        assert_eq!(partial.add(5, &chunks(1, b"")[0]), Some(Vec::new()));
    }
}
