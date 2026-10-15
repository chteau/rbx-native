//! The editor's connection to the relay: one WebSocket on a worker thread,
//! so nothing about the network ever runs on the UI thread.
//!
//! The thread connects to `<relay>/v1/rooms/<room>/ws` with the signed
//! headers from [`crate::auth`] (the key included, so a relay that restarted
//! learns the room again), sends the latest state handed to
//! [`Client::publish`] (and again every [`HEARTBEAT_MS`] even when nothing
//! changed), and reports every [`Snapshot`] it receives. It reconnects after
//! a failure, waiting 1 s, then twice as long each time up to 30 s, and
//! stops when the [`Client`] is dropped.

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, Message, WebSocket};

use crate::relay::{self, RelayKind};
use crate::schema::{Snapshot, UserState, EXPIRY_MS, HEARTBEAT_MS};
use crate::{auth, ntfy};

/// How long a read blocks before the loop checks for something to send.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// How often the ntfy transport publishes-then-polls; presence is best-effort
/// on a shared public server, so a second's latency is fine.
const NTFY_POLL: Duration = Duration::from_secs(1);

/// Unix milliseconds now, or 0 if the clock is before the epoch.
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Where and as whom to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Which transport the relay speaks.
    pub kind: RelayKind,
    /// `https://host[/prefix]` or `http://…` (a local `wrangler dev`); a
    /// trailing `/` is ignored.
    pub relay_url: String,
    /// [`crate::auth::room_id`] of `key`.
    pub room: String,
    /// [`crate::auth::derive_key`] for the place.
    pub key: String,
    pub user_id: u64,
}

/// What the worker reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Snapshot(Snapshot),
    /// The connection dropped or could not be made; it will retry.
    Disconnected(String),
    /// The relay refused us (HTTP 401/403 on the upgrade): wrong key or
    /// clock. Retries continue at the slowest pace.
    Rejected(String),
}

/// The handle; dropping it stops the worker.
pub struct Client {
    states: Sender<UserState>,
}

/// `relay_url` with `https` → `wss`, `http` → `ws`, plus
/// `/v1/rooms/<room>/ws`. `None` for any other scheme.
pub fn ws_url(relay_url: &str, room: &str) -> Option<String> {
    let base = relay_url.trim_end_matches('/');
    let scheme = match base.split_once("://")? {
        ("https", rest) => format!("wss://{rest}"),
        ("http", rest) => format!("ws://{rest}"),
        _ => return None,
    };
    Some(format!("{scheme}/v1/rooms/{room}/ws"))
}

impl Client {
    /// Starts the worker thread.
    pub fn start(config: Config) -> (Client, Receiver<Event>) {
        let (states, state_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        thread::spawn(move || run(config, state_rx, event_tx));
        (Client { states }, events)
    }

    /// Replaces the state the worker sends (latest wins; clamped first).
    pub fn publish(&self, state: UserState) {
        // A send error means the worker already stopped; nothing to do.
        let _ = self.states.send(state);
    }
}

enum Outcome {
    Stopped,
    Failed(String),
    Rejected(String),
}

struct Worker {
    config: Config,
    states: Receiver<UserState>,
    events: Sender<Event>,
    latest: Option<UserState>,
    dirty: bool,
    /// ntfy only: the room rebuilt from the message stream, each user with
    /// the unix-ms we last heard from them.
    ntfy_room: HashMap<u64, (UserState, u64)>,
    /// ntfy only: grows on every room change, like the Worker relay's cursor.
    ntfy_cursor: u64,
    /// ntfy only: the id of the newest message polled, so the next poll asks
    /// only for later ones.
    ntfy_last_id: Option<String>,
}

fn run(config: Config, states: Receiver<UserState>, events: Sender<Event>) {
    let mut worker = Worker {
        config,
        states,
        events,
        latest: None,
        dirty: false,
        ntfy_room: HashMap::new(),
        ntfy_cursor: 0,
        ntfy_last_id: None,
    };
    let mut delay = BACKOFF_START;
    loop {
        let (event, wait) = match worker.session(&mut delay) {
            Outcome::Stopped => return,
            Outcome::Failed(why) => (Event::Disconnected(why), delay),
            Outcome::Rejected(why) => {
                delay = BACKOFF_MAX;
                (Event::Rejected(why), BACKOFF_MAX)
            }
        };
        if worker.events.send(event).is_err() || !worker.wait(wait) {
            return;
        }
        delay = (delay * 2).min(BACKOFF_MAX);
    }
}

impl Worker {
    /// Sleeps for `total` while still collecting published states; false when
    /// the client is gone.
    fn wait(&mut self, total: Duration) -> bool {
        let end = Instant::now() + total;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.states.recv_timeout(left) {
                Ok(state) => self.take(state),
                Err(RecvTimeoutError::Timeout) => return true,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn take(&mut self, state: UserState) {
        self.latest = Some(state);
        self.dirty = true;
    }

    /// Pulls every pending state; false when the client is gone.
    fn drain(&mut self) -> bool {
        loop {
            match self.states.try_recv() {
                Ok(state) => self.take(state),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn session(&mut self, delay: &mut Duration) -> Outcome {
        match self.config.kind {
            RelayKind::Worker => self.worker_session(delay),
            RelayKind::Ntfy => self.ntfy_session(delay),
        }
    }

    fn worker_session(&mut self, delay: &mut Duration) -> Outcome {
        let mut socket = match self.connect() {
            Ok(socket) => socket,
            Err(WsError::Http(response)) if matches!(response.status().as_u16(), 401 | 403) => {
                return Outcome::Rejected(format!("relay refused us: HTTP {}", response.status()));
            }
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        *delay = BACKOFF_START;
        if self.events.send(Event::Connected).is_err() {
            return Outcome::Stopped;
        }
        // Resend whatever we hold so a reconnect restores our presence.
        self.dirty = self.latest.is_some();
        let mut last_sent = Instant::now();
        loop {
            if !self.drain() {
                let _ = socket.close(None);
                return Outcome::Stopped;
            }
            let heartbeat = last_sent.elapsed() >= Duration::from_millis(HEARTBEAT_MS);
            if self.dirty || heartbeat {
                if let Some(state) = &self.latest {
                    let mut state = state.clone();
                    state.clamp();
                    state.user_id = self.config.user_id;
                    let Ok(json) = serde_json::to_string(&state) else {
                        return Outcome::Failed("cannot encode state".into());
                    };
                    if let Err(error) = socket.send(Message::text(json)) {
                        return Outcome::Failed(error.to_string());
                    }
                    last_sent = Instant::now();
                }
                self.dirty = false;
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(snapshot) = serde_json::from_str::<Snapshot>(text.as_str()) {
                        if self.events.send(Event::Snapshot(snapshot)).is_err() {
                            return Outcome::Stopped;
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    return Outcome::Failed("relay closed the connection".into())
                }
                Ok(_) => {}
                Err(WsError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => return Outcome::Failed(error.to_string()),
            }
        }
    }

    /// Presence over a plain ntfy server: publish our signed state to the
    /// room's topic, poll the topic for everyone else's, and rebuild the room
    /// from the stream (ntfy keeps none of it for us). Backs off and retries
    /// on any HTTP failure, like the Worker session.
    fn ntfy_session(&mut self, delay: &mut Duration) -> Outcome {
        let base = self.config.relay_url.trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Outcome::Failed("ntfy relay url must be http(s)".into());
        }
        let topic = relay::ntfy_topic(&self.config.room);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();

        // Prime the room from the recent past before we announce ourselves.
        if let Err(why) = self.ntfy_poll(&agent, &base, &topic) {
            return Outcome::Failed(why);
        }
        *delay = BACKOFF_START;
        if self.events.send(Event::Connected).is_err() {
            return Outcome::Stopped;
        }
        self.dirty = self.latest.is_some();
        let mut last_sent = Instant::now();
        loop {
            if !self.drain() {
                return Outcome::Stopped;
            }
            let heartbeat = last_sent.elapsed() >= Duration::from_millis(HEARTBEAT_MS);
            if self.dirty || heartbeat {
                if let Some(state) = &self.latest {
                    let mut state = state.clone();
                    state.clamp();
                    state.user_id = self.config.user_id;
                    if let Err(why) = self.ntfy_publish(&agent, &base, &topic, &state) {
                        return Outcome::Failed(why);
                    }
                    last_sent = Instant::now();
                }
                self.dirty = false;
            }
            if let Err(why) = self.ntfy_poll(&agent, &base, &topic) {
                return Outcome::Failed(why);
            }
            if !self.wait(NTFY_POLL) {
                return Outcome::Stopped;
            }
        }
    }

    fn ntfy_publish(
        &self,
        agent: &ureq::Agent,
        base: &str,
        topic: &str,
        state: &UserState,
    ) -> Result<(), String> {
        let body = ntfy::fit_encode(
            &self.config.key,
            topic,
            self.config.user_id,
            unix_ms(),
            state,
        )
        .ok_or_else(|| "cannot encode presence".to_string())?;
        let mut resp = agent
            .post(format!("{base}/{topic}"))
            .header("Content-Type", "text/plain")
            .send(body.as_str())
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let _ = resp.body_mut().read_to_string();
        if !(200..300).contains(&status) {
            return Err(format!("ntfy publish: HTTP {status}"));
        }
        Ok(())
    }

    fn ntfy_poll(&mut self, agent: &ureq::Agent, base: &str, topic: &str) -> Result<(), String> {
        let since = self
            .ntfy_last_id
            .clone()
            .unwrap_or_else(|| "10s".to_owned());
        let mut resp = agent
            .get(format!("{base}/{topic}/json"))
            .query("poll", "1")
            .query("since", &since)
            .call()
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("ntfy poll: HTTP {status}"));
        }
        let now = unix_ms();
        let mut changed = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(id) = message.get("id").and_then(|i| i.as_str()) {
                self.ntfy_last_id = Some(id.to_owned());
            }
            if message.get("event").and_then(|e| e.as_str()) != Some("message") {
                continue;
            }
            let Some(body) = message.get("message").and_then(|m| m.as_str()) else {
                continue;
            };
            if let Some(mut state) = ntfy::decode(&self.config.key, topic, now, body) {
                if state.user_id == self.config.user_id {
                    continue;
                }
                state.seen = now;
                self.ntfy_room.insert(state.user_id, (state, now));
                changed = true;
            }
        }
        let before = self.ntfy_room.len();
        self.ntfy_room
            .retain(|_, (_, seen)| now.saturating_sub(*seen) <= EXPIRY_MS);
        changed |= self.ntfy_room.len() != before;
        if changed {
            self.ntfy_cursor += 1;
            let users = self.ntfy_room.values().map(|(s, _)| s.clone()).collect();
            let snapshot = Snapshot {
                cursor: self.ntfy_cursor,
                users,
            };
            let _ = self.events.send(Event::Snapshot(snapshot));
        }
        Ok(())
    }

    fn connect(&self) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, WsError> {
        let url = ws_url(&self.config.relay_url, &self.config.room)
            .ok_or_else(|| WsError::Url(tungstenite::error::UrlError::UnsupportedUrlScheme))?;
        let mut request = url.into_client_request()?;
        let path = format!("/v1/rooms/{}/ws", self.config.room);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        for (name, value) in auth::headers(
            &self.config.key,
            "GET",
            &path,
            self.config.user_id,
            ts,
            b"",
            true,
        ) {
            if let Ok(value) = HeaderValue::from_str(&value) {
                request
                    .headers_mut()
                    .insert(HeaderName::from_static_lower(name), value);
            }
        }
        let (socket, _) = tungstenite::connect(request)?;
        match socket.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(READ_TIMEOUT))?,
            MaybeTlsStream::Rustls(stream) => {
                stream.get_ref().set_read_timeout(Some(READ_TIMEOUT))?
            }
            _ => {}
        }
        Ok(socket)
    }
}

trait FromStaticLower {
    fn from_static_lower(name: &'static str) -> HeaderName;
}

impl FromStaticLower for HeaderName {
    /// Header names here are fixed ASCII constants; `http` wants them lower
    /// case on the wire.
    fn from_static_lower(name: &'static str) -> HeaderName {
        HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes())
            .expect("presence header names are valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Client as Editor, SCHEMA_VERSION};
    use std::net::TcpListener;
    use tungstenite::handshake::server::{ErrorResponse, Request, Response};

    #[test]
    fn ws_url_cases() {
        assert_eq!(
            ws_url("https://r.example", "abc").unwrap(),
            "wss://r.example/v1/rooms/abc/ws"
        );
        assert_eq!(
            ws_url("http://127.0.0.1:8787", "abc").unwrap(),
            "ws://127.0.0.1:8787/v1/rooms/abc/ws"
        );
        assert_eq!(
            ws_url("https://r.example/p/", "abc").unwrap(),
            "wss://r.example/p/v1/rooms/abc/ws"
        );
        assert_eq!(ws_url("ftp://r.example", "abc"), None);
    }

    fn state(user_id: u64) -> UserState {
        UserState {
            v: SCHEMA_VERSION,
            user_id,
            name: "Ann".into(),
            color: "#e8590c".into(),
            client: Editor::RbxNative,
            camera: None,
            selection: vec![],
            script: None,
            edits: vec![],
            seen: 0,
        }
    }

    #[test]
    // The handshake callback's `Result` shape is tungstenite's, not ours.
    #[allow(clippy::result_large_err)]
    fn connects_publishes_and_receives_snapshot() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let key = "k".repeat(64);
        let (hdr_tx, hdr_rx) = mpsc::channel();
        let (got_tx, got_rx) = mpsc::channel();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let callback = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                let get = |n: &str| {
                    req.headers()
                        .get(n)
                        .map(|v| v.to_str().unwrap().to_string())
                };
                hdr_tx
                    .send((
                        req.uri().path().to_string(),
                        get("x-presence-user"),
                        get("x-presence-time"),
                        get("x-presence-sig"),
                        get("x-presence-key"),
                    ))
                    .unwrap();
                Ok(resp)
            };
            let mut ws = tungstenite::accept_hdr(stream, callback).unwrap();
            let Message::Text(text) = ws.read().unwrap() else {
                panic!("expected text");
            };
            let got: UserState = serde_json::from_str(text.as_str()).unwrap();
            got_tx.send(got).unwrap();
            let snapshot = Snapshot {
                cursor: 3,
                users: vec![state(9)],
            };
            ws.send(Message::text(serde_json::to_string(&snapshot).unwrap()))
                .unwrap();
            let _ = ws.read();
        });

        let (client, events) = Client::start(Config {
            kind: RelayKind::Worker,
            relay_url: format!("http://127.0.0.1:{port}"),
            room: "room1".into(),
            key: key.clone(),
            user_id: 42,
        });
        client.publish(state(1));

        let (path, user, time, sig, sent_key) =
            hdr_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(path, "/v1/rooms/room1/ws");
        assert_eq!(user.as_deref(), Some("42"));
        assert_eq!(sent_key.as_deref(), Some(key.as_str()));
        let ts: u64 = time.unwrap().parse().unwrap();
        let canon = auth::canonical("GET", &path, 42, ts, b"");
        assert_eq!(sig.unwrap(), auth::sign(&key, &canon));

        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Connected
        );
        let got = got_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got.user_id, 42);
        match events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Snapshot(s) => assert_eq!(s.cursor, 3),
            other => panic!("unexpected {other:?}"),
        }
    }
}
