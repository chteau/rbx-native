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
//!
//! The hosted relay ([`crate::host`]) and the Worker (`relay/`) both serve
//! that route. A plain ntfy server has no routes of ours, so over
//! [`RelayKind::Ntfy`] each end publishes to the topic of the end it
//! addresses (`rbxnp-<room>-play-<16 hex>`, all zeros for everyone) and
//! streams its own and the everyone topic: a message there is lines of
//! `<from, 16 hex> <base64 of a sealed chunk>`, chunks small enough for
//! ntfy's 4 KB message limit. The seal is the same, so ntfy can't read or
//! forge them either; it only needs to let one end publish fast enough
//! (a self-hosted server: public ntfy.sh allows far too few messages).

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use base64::Engine as _;

use crate::client::connect_route;
use crate::crypt;
use crate::relay::RelayKind;

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
/// The most of a message one ntfy line carries: sealed and in base64 it
/// stays under ntfy's 4096 byte message limit with room to spare.
const NTFY_CHUNK_BYTES: usize = 2048;
/// The most one ntfy message (lines of chunks) holds.
const NTFY_MESSAGE_BYTES: usize = 3800;

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
    /// The room's transport: the play route on the hosted relay or the
    /// Worker, or topics on a plain ntfy server.
    pub kind: RelayKind,
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
    /// Message bytes handed to [`Link::send`] and not written yet: what a
    /// slow link is behind by.
    pub queued: AtomicU64,
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
            .spawn(move || match config.kind {
                RelayKind::Ntfy => ntfy::run(config, out_rx, events_tx, thread_stop, thread_stats),
                RelayKind::Hosted | RelayKind::Worker => {
                    run(config, out_rx, events_tx, thread_stop, thread_stats)
                }
            })
            .expect("a thread for Local Run play");
        (Link { out, stop, stats }, events)
    }

    /// Sends `bytes` to end `to` (0: everyone else in the room). Dropped
    /// while disconnected.
    pub fn send(&self, to: u64, bytes: Vec<u8>) {
        self.stats
            .queued
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        let _ = self.out.send((to, bytes));
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Bytes sent and not on the wire yet.
    pub fn backlog(&self) -> u64 {
        self.stats.queued.load(Ordering::Relaxed)
    }
}

/// A message left the queue (written, or given up on).
fn dequeued(stats: &Stats, bytes: &[u8]) {
    let mut q = stats.queued.load(Ordering::Relaxed);
    while let Err(now) = stats.queued.compare_exchange_weak(
        q,
        q.saturating_sub(bytes.len() as u64),
        Ordering::Relaxed,
        Ordering::Relaxed,
    ) {
        q = now;
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// `bytes` cut into chunks of at most `size`, each `id`, its index and
/// the count first.
fn chunks(id: u32, bytes: &[u8], size: usize) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = if bytes.is_empty() {
        vec![&[]]
    } else {
        bytes.chunks(size).collect()
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
        while let Ok((_, bytes)) = out.try_recv() {
            dequeued(&stats, &bytes);
        }
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
            for chunk in chunks(*next_id, &bytes, CHUNK_BYTES) {
                let mut message = to.to_be_bytes().to_vec();
                message.extend(crypt::seal(&config.enc_key, &aad, &chunk));
                stats
                    .sent
                    .fetch_add(message.len() as u64, Ordering::Relaxed);
                if let Err(e) = crate::tick::send(socket, Message::binary(message)) {
                    dequeued(stats, &bytes);
                    return Some(e.to_string());
                }
            }
            dequeued(stats, &bytes);
            *next_id = next_id.wrapping_add(1);
        }
        if let Err(e) = crate::tick::flush(socket) {
            return Some(e.to_string());
        }
        match socket.read() {
            Ok(Message::Binary(data)) if data.len() > 8 => {
                stats
                    .received
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
                let from = u64::from_be_bytes(data[..8].try_into().unwrap_or_default());
                let sealed = &data[8..];
                let chunk = open_chunk(config, from, sealed);
                if let Some(bytes) = chunk.and_then(|c| partial.add(from, &c)) {
                    if events.send(Event::Message { from, bytes }).is_err() {
                        return None;
                    }
                }
            }
            Ok(Message::Close(_)) => return Some("the relay closed the connection".into()),
            Ok(_) => {}
            Err(e) if crate::tick::nothing_read(&e, TICK) => {}
            Err(e) => return Some(e.to_string()),
        }
    }
}

/// Opens a sealed chunk from `from`, addressed to `pid` or to everyone.
fn open_chunk(config: &Config, from: u64, sealed: &[u8]) -> Option<Vec<u8>> {
    [config.pid, 0].into_iter().find_map(|to| {
        crypt::open(
            &config.enc_key,
            &crypt::play_aad(&config.room, from, to),
            sealed,
        )
    })
}

/// Play over a plain ntfy server (see the module docs).
mod ntfy {
    use std::io::{BufRead, BufReader};

    use super::*;

    /// The topic messages to `pid` are published to (0: everyone).
    pub(super) fn topic(room: &str, pid: u64) -> String {
        format!("{}-play-{pid:016x}", crate::relay::ntfy_topic(room))
    }

    fn agent(timeout: Option<Duration>) -> ureq::Agent {
        rbx_net::config()
            .http_status_as_error(false)
            .timeout_global(timeout)
            .build()
            .into()
    }

    /// The lines one message to `to` goes as.
    fn lines(config: &Config, to: u64, id: u32, bytes: &[u8]) -> Vec<String> {
        let aad = crypt::play_aad(&config.room, config.pid, to);
        chunks(id, bytes, NTFY_CHUNK_BYTES)
            .iter()
            .map(|chunk| {
                let sealed = crypt::seal(&config.enc_key, &aad, chunk);
                format!(
                    "{:016x} {}",
                    config.pid,
                    base64::engine::general_purpose::STANDARD.encode(sealed)
                )
            })
            .collect()
    }

    pub(super) fn run(
        config: Config,
        out: Receiver<(u64, Vec<u8>)>,
        events: Sender<Event>,
        stop: Arc<AtomicBool>,
        stats: Arc<Stats>,
    ) {
        let base = config.relay_url.trim_end_matches('/').to_owned();
        {
            let (config, events, stop, stats) =
                (config.clone(), events.clone(), stop.clone(), stats.clone());
            let base = base.clone();
            thread::Builder::new()
                .name("presence-play-ntfy".into())
                .spawn(move || listen(&config, &base, &events, &stop, &stats))
                .expect("a thread for Local Run play over ntfy");
        }
        let publisher = agent(Some(Duration::from_secs(10)));
        let mut next_id = 0u32;
        while !stop.load(Ordering::SeqCst) {
            let first = match out.recv_timeout(Duration::from_millis(50)) {
                Ok(first) => first,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            // Everything waiting goes now, a message per address holding
            // as many lines as fit.
            let mut waiting = vec![first];
            while let Ok(more) = out.try_recv() {
                waiting.push(more);
            }
            let mut bodies: Vec<(u64, String)> = Vec::new();
            for (to, bytes) in &waiting {
                for line in lines(&config, *to, next_id, bytes) {
                    match bodies.last_mut() {
                        Some((at, body))
                            if *at == *to && body.len() + line.len() < NTFY_MESSAGE_BYTES =>
                        {
                            body.push('\n');
                            body.push_str(&line);
                        }
                        _ => bodies.push((*to, line)),
                    }
                }
                next_id = next_id.wrapping_add(1);
            }
            for (to, body) in bodies {
                let sent = publisher
                    .post(format!("{base}/{}", topic(&config.room, to)))
                    .header("Content-Type", "text/plain")
                    .send(body.as_str());
                if sent.is_ok_and(|r| r.status().is_success()) {
                    stats.sent.fetch_add(body.len() as u64, Ordering::Relaxed);
                }
            }
            for (_, bytes) in &waiting {
                dequeued(&stats, bytes);
            }
        }
    }

    /// Streams this end's topic and the everyone topic until stopped,
    /// reconnecting with backoff.
    fn listen(
        config: &Config,
        base: &str,
        events: &Sender<Event>,
        stop: &AtomicBool,
        stats: &Stats,
    ) {
        let topics = format!(
            "{},{}",
            topic(&config.room, config.pid),
            topic(&config.room, 0)
        );
        let listener = agent(None);
        let mut delay = BACKOFF_START;
        while !stop.load(Ordering::SeqCst) {
            let why = match listener.get(format!("{base}/{topics}/json")).call() {
                Ok(response) if response.status().is_success() => {
                    delay = BACKOFF_START;
                    if events.send(Event::Connected).is_err() {
                        return;
                    }
                    let mut partial = Partial::default();
                    let reader = BufReader::new(response.into_body().into_reader());
                    let mut why = "the ntfy stream ended".to_owned();
                    for line in reader.lines() {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        let Ok(line) = line else {
                            why = "the ntfy stream broke".to_owned();
                            break;
                        };
                        stats
                            .received
                            .fetch_add(line.len() as u64, Ordering::Relaxed);
                        for (from, bytes) in read(config, &line, &mut partial) {
                            if events.send(Event::Message { from, bytes }).is_err() {
                                return;
                            }
                        }
                    }
                    why
                }
                Ok(response) => format!("ntfy: HTTP {}", response.status().as_u16()),
                Err(e) => e.to_string(),
            };
            if events.send(Event::Disconnected(why)).is_err() {
                return;
            }
            nap(stop, delay);
            delay = (delay * 2).min(BACKOFF_MAX);
        }
    }

    /// The whole messages one line of ntfy's JSON stream completes.
    pub(super) fn read(config: &Config, line: &str, partial: &mut Partial) -> Vec<(u64, Vec<u8>)> {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            return Vec::new();
        };
        if event.get("event").and_then(|e| e.as_str()) != Some("message") {
            return Vec::new();
        }
        let Some(text) = event.get("message").and_then(|m| m.as_str()) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| {
                let (from, sealed) = line.split_once(' ')?;
                let from = u64::from_str_radix(from, 16).ok()?;
                if from == config.pid {
                    return None;
                }
                let sealed = base64::engine::general_purpose::STANDARD
                    .decode(sealed)
                    .ok()?;
                let chunk = open_chunk(config, from, &sealed)?;
                Some((from, partial.add(from, &chunk)?))
            })
            .collect()
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
            kind: RelayKind::Hosted,
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
        let parts = chunks(9, &bytes, CHUNK_BYTES);
        assert_eq!(parts.len(), 2);
        let mut partial = Partial::default();
        assert_eq!(partial.add(5, &parts[1]), None);
        assert_eq!(partial.add(5, &parts[0]), Some(bytes));
        assert_eq!(
            partial.add(5, &chunks(1, b"", CHUNK_BYTES)[0]),
            Some(Vec::new())
        );
    }

    /// A stand-in ntfy server: `POST /<topic>` hands the body to every
    /// stream open on that topic; `GET /<a>,<b>/json` streams them as
    /// ntfy's JSON lines, an `open` event first.
    fn fake_ntfy() -> String {
        use std::io::{BufRead, BufReader, Read, Write};
        type Streams = Arc<std::sync::Mutex<Vec<(Vec<String>, TcpStream)>>>;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let streams: Streams = Arc::default();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let streams = streams.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let mut parts = line.split_whitespace();
                    let method = parts.next().unwrap_or("").to_owned();
                    let path = parts
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .to_owned();
                    let mut len = 0;
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    if method == "POST" {
                        let mut body = vec![0; len];
                        reader.read_exact(&mut body).unwrap();
                        let event = serde_json::json!({
                            "event": "message", "topic": path,
                            "message": String::from_utf8(body).unwrap(),
                        })
                        .to_string()
                            + "\n";
                        for (topics, out) in streams.lock().unwrap().iter_mut() {
                            if topics.contains(&path) {
                                let _ = out.write_all(event.as_bytes());
                            }
                        }
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
                        );
                    } else {
                        let topics = path.trim_end_matches("/json");
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\n\r\n{}\n",
                            serde_json::json!({ "event": "open", "topic": topics })
                        );
                        let topics = topics.split(',').map(str::to_owned).collect();
                        streams.lock().unwrap().push((topics, stream));
                    }
                });
            }
        });
        url
    }

    /// Two ends over a plain ntfy server: addressed and broadcast messages,
    /// a big one cut into ntfy-sized lines, and nothing heard back from
    /// oneself.
    #[test]
    fn ends_reach_each_other_through_ntfy_sealed() {
        let url = fake_ntfy();
        let key = crate::auth::derive_key("secret", 1);
        let ntfy = |pid| Config {
            kind: RelayKind::Ntfy,
            ..config(&url, &key, "secret", pid)
        };
        let (a, a_events) = Link::start(ntfy(1));
        let (b, b_events) = Link::start(ntfy(2));
        for events in [&a_events, &b_events] {
            assert_eq!(next(events), Event::Connected);
        }
        a.send(2, b"to b".to_vec());
        assert_eq!(message(&b_events), (1, b"to b".to_vec()));
        b.send(0, b"to everyone".to_vec());
        assert_eq!(message(&a_events), (2, b"to everyone".to_vec()));
        let big: Vec<u8> = (0..NTFY_CHUNK_BYTES * 5 + 3).map(|i| i as u8).collect();
        a.send(2, big.clone());
        assert_eq!(message(&b_events), (1, big));
        assert!(
            a_events.recv_timeout(Duration::from_millis(300)).is_err(),
            "a hears nothing of its own"
        );
        assert_eq!(a.backlog(), 0, "everything left");
    }
}
