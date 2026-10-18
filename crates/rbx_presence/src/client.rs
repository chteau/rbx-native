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
use crate::schema::{
    self, Client as Editor, SealedSnapshot, Snapshot, UserState, EXPIRY_MS, HEARTBEAT_MS,
};
use crate::{announce, auth, host, ntfy, seal};

/// What a client reports when the relay refuses its wire version.
pub const VERSION_MISMATCH: &str = "The relay runs a different Team Presence version; update rbx-native, the plugin and the relay together.";

/// Opens every user of a relay snapshot, dropping those that fail to open.
pub fn open_snapshot(enc_key: &[u8; 32], room: &str, sealed: &SealedSnapshot) -> Snapshot {
    Snapshot {
        cursor: sealed.cursor,
        users: sealed
            .users
            .iter()
            .filter_map(|user| seal::open_state(enc_key, room, user))
            .collect(),
    }
}

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

/// A public tunnel to a loopback port, kept open while it lives.
pub trait Tunnel: Send {
    /// Whether the tunnel is still up (its process has not exited).
    fn alive(&mut self) -> bool;
}

/// Opens a [`Tunnel`] to `127.0.0.1:<port>` and returns its public
/// `https://` URL. Supplied by the editor, which ships `cloudflared`.
pub type OpenTunnel = dyn Fn(u16) -> Result<(String, Box<dyn Tunnel>), String> + Send + Sync;

/// How a [`RelayKind::Hosted`] client may host the room itself; without
/// one (or when it fails) the client only joins an announced host.
#[derive(Clone)]
pub struct Hosting(pub std::sync::Arc<OpenTunnel>);

impl std::fmt::Debug for Hosting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hosting(..)")
    }
}

impl PartialEq for Hosting {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Hosting {}

/// Where and as whom to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Which transport the relay speaks.
    pub kind: RelayKind,
    /// `https://host[/prefix]` or `http://…` (a local `wrangler dev`); a
    /// trailing `/` is ignored. For [`RelayKind::Hosted`], the ntfy server
    /// the host's address is announced on.
    pub relay_url: String,
    /// [`crate::auth::room_id`] of `key`.
    pub room: String,
    /// [`crate::auth::derive_key`] for the place.
    pub key: String,
    /// [`crate::crypt::enc_key`] for the place; never sent anywhere.
    pub enc_key: [u8; 32],
    pub user_id: u64,
    /// [`RelayKind::Hosted`] only: lets this client host the room.
    pub hosting: Option<Hosting>,
}

/// How a [`RelayKind::Hosted`] client is connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// This client runs the relay, public at this address.
    Hosting(String),
    /// Connected to the relay another rbx-native user hosts there.
    Joined(String),
    /// Nobody hosts the room yet, and this client cannot.
    Waiting,
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
    /// [`RelayKind::Hosted`]: how this client reaches the room now.
    Route(Route),
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
    /// Keyed on [`schema::sender_key`]: one user can be in Studio and
    /// rbx-native at once.
    ntfy_room: HashMap<(u64, Editor), (UserState, u64)>,
    /// ntfy only: grows on every room change, like the Worker relay's cursor.
    ntfy_cursor: u64,
    /// ntfy only: the id of the newest message polled, so the next poll asks
    /// only for later ones.
    ntfy_last_id: Option<String>,
    /// Hosted only: the relay this client runs, while it hosts.
    hosted: Option<Hosted>,
    /// Hosted only: the announced relay this client joined.
    joined: Option<String>,
}

/// The relay this client runs for the room, and its public tunnel.
struct Hosted {
    host: host::Host,
    tunnel: Box<dyn Tunnel>,
    public: String,
    /// When the address was last announced (unix ms).
    announced: u64,
}

/// How often a hosted-room client re-checks the announcements (a host to
/// re-announce and to yield to a newer host, a guest to follow a move).
const ROUTE_CHECK: Duration = Duration::from_secs(60);

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
        hosted: None,
        joined: None,
    };
    let mut delay = BACKOFF_START;
    loop {
        let (event, wait) = match worker.session(&mut delay) {
            Outcome::Stopped => {
                worker.stop_hosting();
                return;
            }
            Outcome::Failed(why) => (Event::Disconnected(why), delay),
            Outcome::Rejected(why) => {
                delay = BACKOFF_MAX;
                (Event::Rejected(why), BACKOFF_MAX)
            }
        };
        if worker.events.send(event).is_err() || !worker.wait(wait) {
            worker.stop_hosting();
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
            RelayKind::Hosted => self.hosted_session(delay),
            RelayKind::Worker => {
                let url = self.config.relay_url.clone();
                self.worker_session(&url, delay)
            }
            RelayKind::Ntfy => self.ntfy_session(delay),
        }
    }

    /// A hosted room: join the announced host if it answers, else host it
    /// ourselves (when we can), then talk to that relay like a Worker.
    fn hosted_session(&mut self, delay: &mut Duration) -> Outcome {
        if self.hosted.is_none() {
            let ntfy = self.config.relay_url.clone();
            match announce::find(&ntfy, &self.config.key, &self.config.room) {
                Err(why) => return Outcome::Failed(why),
                Ok(Some(found)) if reachable(&found.url) => {
                    self.joined = Some(found.url.clone());
                    if self
                        .events
                        .send(Event::Route(Route::Joined(found.url.clone())))
                        .is_err()
                    {
                        return Outcome::Stopped;
                    }
                    return self.worker_session(&found.url, delay);
                }
                Ok(_) => {
                    self.joined = None;
                    let Some(hosting) = self.config.hosting.clone() else {
                        if self.events.send(Event::Route(Route::Waiting)).is_err() {
                            return Outcome::Stopped;
                        }
                        return Outcome::Failed("no rbx-native collaborator is hosting yet".into());
                    };
                    if let Err(why) = self.start_hosting(&hosting) {
                        return Outcome::Failed(why);
                    }
                }
            }
        }
        let Some(local) = self.hosted.as_ref().map(|h| h.host.local_url()) else {
            return Outcome::Failed("hosting stopped".into());
        };
        self.worker_session(&local, delay)
    }

    fn start_hosting(&mut self, hosting: &Hosting) -> Result<(), String> {
        let host = host::Host::start(&self.config.key).map_err(|e| e.to_string())?;
        let (public, tunnel) = (hosting.0)(host.port())?;
        if !announce::allowed_url(&public) {
            return Err(format!("the tunnel gave an unusable address: {public}"));
        }
        // Collaborators look the address up the moment it is announced; one
        // asked before the record exists caches "no such host" for many
        // minutes. Announce only once public DNS has it.
        wait_until_resolvable(&public, Duration::from_secs(45));
        announce::publish(
            &self.config.relay_url,
            &self.config.key,
            &self.config.room,
            &public,
        )?;
        let _ = self
            .events
            .send(Event::Route(Route::Hosting(public.clone())));
        self.hosted = Some(Hosted {
            host,
            tunnel,
            public,
            announced: unix_ms(),
        });
        Ok(())
    }

    /// Leaves the room's hosting: says goodbye so guests look again at once,
    /// then closes the tunnel and the relay.
    fn stop_hosting(&mut self) {
        if let Some(hosted) = self.hosted.take() {
            let _ = announce::publish(
                &self.config.relay_url,
                &self.config.key,
                &self.config.room,
                "",
            );
            drop(hosted);
        }
    }

    /// The periodic hosted-room check; `Some` ends the session (to rejoin or
    /// re-host): a host whose tunnel died or that a newer host replaced, or a
    /// guest whose host moved or left.
    fn check_route(&mut self) -> Option<Outcome> {
        let found = announce::find(&self.config.relay_url, &self.config.key, &self.config.room);
        if let Some(hosted) = &mut self.hosted {
            if !hosted.tunnel.alive() {
                self.hosted = None;
                return Some(Outcome::Failed("the tunnel closed".into()));
            }
            if let Ok(Some(newer)) = &found {
                if newer.url != hosted.public && newer.at > hosted.announced {
                    // Two hosted at once: the newer one keeps the room.
                    self.hosted = None;
                    return Some(Outcome::Failed(
                        "another rbx-native user hosts the room".into(),
                    ));
                }
            }
            let now = unix_ms();
            let due = now.saturating_sub(hosted.announced) >= announce::REANNOUNCE_MS;
            if due
                && announce::publish(
                    &self.config.relay_url,
                    &self.config.key,
                    &self.config.room,
                    &hosted.public,
                )
                .is_ok()
            {
                hosted.announced = now;
            }
            return None;
        }
        if let Some(joined) = &self.joined {
            match found {
                Ok(Some(current)) if &current.url == joined => None,
                Ok(_) => Some(Outcome::Failed("the room's host moved".into())),
                Err(_) => None,
            }
        } else {
            None
        }
    }

    fn worker_session(&mut self, url: &str, delay: &mut Duration) -> Outcome {
        let mut socket = match self.connect(url) {
            Ok(socket) => socket,
            Err(WsError::Http(response)) if matches!(response.status().as_u16(), 401 | 403) => {
                return Outcome::Rejected(format!("relay refused us: HTTP {}", response.status()));
            }
            Err(WsError::Http(response))
                if response.status().as_u16() == 400
                    && response
                        .body()
                        .as_deref()
                        .is_some_and(|b| String::from_utf8_lossy(b).contains("version")) =>
            {
                return Outcome::Rejected(VERSION_MISMATCH.into());
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
        let mut last_check = Instant::now();
        loop {
            if !self.drain() {
                let _ = socket.close(None);
                return Outcome::Stopped;
            }
            if self.config.kind == RelayKind::Hosted && last_check.elapsed() >= ROUTE_CHECK {
                last_check = Instant::now();
                if let Some(outcome) = self.check_route() {
                    let _ = socket.close(None);
                    return outcome;
                }
            }
            let heartbeat = last_sent.elapsed() >= Duration::from_millis(HEARTBEAT_MS);
            if self.dirty || heartbeat {
                if let Some(state) = &self.latest {
                    let mut state = state.clone();
                    state.clamp();
                    state.user_id = self.config.user_id;
                    let sealed = seal::seal_state(&self.config.enc_key, &self.config.room, &state);
                    let Ok(json) = serde_json::to_string(&sealed) else {
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
                    if let Ok(sealed) = serde_json::from_str::<SealedSnapshot>(text.as_str()) {
                        let snapshot =
                            open_snapshot(&self.config.enc_key, &self.config.room, &sealed);
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
                        if why == VERSION_MISMATCH {
                            return Outcome::Rejected(why);
                        }
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
            &self.config.enc_key,
            &self.config.room,
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
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        if status == 400 && text.contains("version") {
            return Err(VERSION_MISMATCH.into());
        }
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
            if let Some(mut state) = ntfy::decode(
                &self.config.key,
                &self.config.enc_key,
                &self.config.room,
                topic,
                now,
                body,
            ) {
                if schema::is_own(&state, self.config.user_id) {
                    continue;
                }
                state.seen = now;
                self.ntfy_room
                    .insert(schema::sender_key(&state), (state, now));
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

    fn connect(&self, relay_url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, WsError> {
        let url = ws_url(relay_url, &self.config.room)
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

/// Waits (at most `limit`) until `url`'s host has an address in public DNS,
/// asking Cloudflare's DNS-over-HTTPS resolver so this machine's own resolver
/// never caches an early "no such host". Loopback addresses return at once.
fn wait_until_resolvable(url: &str, limit: Duration) {
    let Some(host) = url
        .strip_prefix("https://")
        .map(|rest| rest.split(['/', ':']).next().unwrap_or(rest))
    else {
        return;
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let end = Instant::now() + limit;
    while Instant::now() < end {
        let answered = agent
            .get("https://cloudflare-dns.com/dns-query")
            .query("name", host)
            .query("type", "A")
            .header("Accept", "application/dns-json")
            .call()
            .ok()
            .and_then(|mut resp| resp.body_mut().read_to_string().ok())
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
            .is_some_and(|json| {
                json.get("Status").and_then(|s| s.as_u64()) == Some(0)
                    && json
                        .get("Answer")
                        .and_then(|a| a.as_array())
                        .is_some_and(|a| !a.is_empty())
            });
        if answered {
            return;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

/// Whether a relay answers its health check (a host that crashed leaves its
/// last announcement behind).
fn reachable(url: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    agent
        .get(format!("{}/health", url.trim_end_matches('/')))
        .call()
        .is_ok_and(|resp| resp.status().as_u16() == 200)
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
    use crate::schema::SCHEMA_VERSION;
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
            let enc_key = crate::crypt::enc_key("s", 1);
            let sealed: schema::Sealed = serde_json::from_str(text.as_str()).unwrap();
            let got = seal::open_state(&enc_key, "room1", &sealed).unwrap();
            got_tx.send(got).unwrap();
            let mut forged = seal::seal_state(&crate::crypt::enc_key("s", 2), "room1", &state(8));
            forged.seen = 1;
            let snapshot = SealedSnapshot {
                cursor: 3,
                users: vec![seal::seal_state(&enc_key, "room1", &state(9)), forged],
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
            enc_key: crate::crypt::enc_key("s", 1),
            user_id: 42,
            hosting: None,
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
            Event::Snapshot(s) => {
                assert_eq!(s.cursor, 3);
                assert_eq!(s.users, vec![state(9)]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
