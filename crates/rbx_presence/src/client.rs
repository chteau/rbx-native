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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Condvar, Mutex, PoisonError};
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
use going::{Going, Topic};

mod going;

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
/// The first wait before trying a tunnel again after it failed (for any
/// reason but a rate limit, which says how long), doubling up to
/// [`TUNNEL_RETRY_MAX`].
const TUNNEL_RETRY_START: Duration = Duration::from_secs(15);
const TUNNEL_RETRY_MAX: Duration = Duration::from_secs(10 * 60);

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
pub type OpenTunnel = dyn Fn(u16) -> Result<(String, Box<dyn Tunnel>), TunnelRefused> + Send + Sync;

/// Why a host's tunnel didn't open (or going public through it failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelRefused {
    pub why: String,
    /// Set when the tunnel service rate limits this network: no new tunnel
    /// before this much time has passed.
    pub retry_in: Option<Duration>,
}

impl From<String> for TunnelRefused {
    fn from(why: String) -> Self {
        TunnelRefused {
            why,
            retry_in: None,
        }
    }
}

impl From<&str> for TunnelRefused {
    fn from(why: &str) -> Self {
        why.to_owned().into()
    }
}

/// See [`Event::RemotePaused`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paused {
    pub why: String,
    /// The tunnel service rate limits this network.
    pub rate_limited: bool,
    /// When the tunnel is tried again (unix ms).
    pub retry_at_ms: u64,
}

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
    /// The place, for [`crate::beacon`] (hosted rooms only).
    pub place_id: u64,
    /// This editor's [`crate::beacon`] sender id, the same every session.
    pub beacon_from: String,
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
    /// [`RelayKind::Hosted`]: the client ("studio" or "rbx-native") of an
    /// editor that has the place open with a different presence key, or
    /// `None` once there is none (see [`crate::beacon`]).
    KeyConflict(Option<String>),
    /// [`RelayKind::Hosted`], while hosting: the tunnel didn't open (or
    /// closed), so only Studio on this computer, on the loopback port,
    /// reaches this host until the next try. The next [`Event::Route`]
    /// supersedes it.
    RemotePaused(Paused),
}

/// The handle; dropping it stops the worker.
pub struct Client {
    states: Sender<UserState>,
}

/// How many workers are still running, so quitting can wait for them.
static RUNNING: Mutex<usize> = Mutex::new(0);
static STOPPED: Condvar = Condvar::new();
/// Set once the app quits: every worker leaves its room.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// For quitting the app: makes every worker leave its room (a host says
/// goodbye on the announce topic and closes its tunnel) and waits up to
/// `limit` for them. The process ending would otherwise cut them off first
/// and leave guests on a dead address. Keep `limit` short: a worker stuck
/// on an unreachable ntfy is simply left behind. True when all of them left.
pub fn leave_all(limit: Duration) -> bool {
    QUITTING.store(true, Ordering::Relaxed);
    let end = Instant::now() + limit;
    let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    while *running > 0 {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        running = STOPPED
            .wait_timeout(running, left)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    true
}

/// `relay_url` with `https` → `wss`, `http` → `ws`, plus
/// `/v1/rooms/<room>/ws`. `None` for any other scheme.
pub fn ws_url(relay_url: &str, room: &str) -> Option<String> {
    route_url(relay_url, room, "ws")
}

/// [`ws_url`] for any of the room's WebSocket routes (`route` may carry a
/// query).
fn route_url(relay_url: &str, room: &str, route: &str) -> Option<String> {
    let base = relay_url.trim_end_matches('/');
    let scheme = match base.split_once("://")? {
        ("https", rest) => format!("wss://{rest}"),
        ("http", rest) => format!("ws://{rest}"),
        _ => return None,
    };
    Some(format!("{scheme}/v1/rooms/{room}/{route}"))
}

/// Opens the room's WebSocket `route` (`ws`, `play?pid=…`) on `relay_url`
/// as `user_id`, with the signed headers (the key included); reads time
/// out after `read_timeout`.
pub(crate) fn connect_route(
    relay_url: &str,
    room: &str,
    key: &str,
    user_id: u64,
    route: &str,
    read_timeout: Duration,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, WsError> {
    let url = route_url(relay_url, room, route)
        .ok_or_else(|| WsError::Url(tungstenite::error::UrlError::UnsupportedUrlScheme))?;
    let mut request = url.into_client_request()?;
    if request.uri().host().is_some_and(rbx_net::blocks) {
        return Err(WsError::Io(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            format!("{}: {}", rbx_net::OFFLINE, rbx_net::status().reason()),
        )));
    }
    let path = format!("/v1/rooms/{room}/{route}");
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    for (name, value) in auth::headers(key, "GET", &path, user_id, ts, b"", true) {
        if let Ok(value) = HeaderValue::from_str(&value) {
            request
                .headers_mut()
                .insert(HeaderName::from_static_lower(name), value);
        }
    }
    let (socket, _) = tungstenite::connect(request)?;
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => crate::tick::set(stream, read_timeout)?,
        MaybeTlsStream::Rustls(stream) => crate::tick::set(stream.get_ref(), read_timeout)?,
        _ => {}
    }
    Ok(socket)
}

impl Client {
    /// Starts the worker thread.
    pub fn start(config: Config) -> (Client, Receiver<Event>) {
        let (states, state_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        thread::spawn(move || {
            run(config, state_rx, event_tx);
            *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            STOPPED.notify_all();
        });
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
    /// Hosted only: when this client's [`crate::beacon`] was last posted
    /// and read, and what the last read found.
    beacon_posted: Option<Instant>,
    beacon_read: Option<Instant>,
    key_conflict: Option<String>,
}

/// The relay this client runs for the room, and how far it is public.
struct Hosted {
    host: host::Host,
    /// Until the host is public: the thread making it so.
    going: Option<Going>,
    /// Once announced: the tunnel and its address.
    public: Option<Announced>,
    /// After the tunnel failed: when to try again, and the wait after the
    /// next failure.
    retry_at: Option<Instant>,
    retry_wait: Duration,
}

struct Announced {
    tunnel: Box<dyn Tunnel>,
    url: String,
    /// When the address was last announced (unix ms).
    at: u64,
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
        beacon_posted: None,
        beacon_read: None,
        key_conflict: None,
    };
    let mut delay = BACKOFF_START;
    loop {
        let (event, wait) = match worker.session(&mut delay) {
            Outcome::Stopped => {
                worker.leave();
                return;
            }
            Outcome::Failed(why) => (Event::Disconnected(why), delay),
            Outcome::Rejected(why) => {
                delay = BACKOFF_MAX;
                (Event::Rejected(why), BACKOFF_MAX)
            }
        };
        if worker.events.send(event).is_err() || !worker.wait(wait) {
            worker.leave();
            return;
        }
        delay = (delay * 2).min(BACKOFF_MAX);
    }
}

impl Worker {
    /// Sleeps for `total` while still collecting published states; false when
    /// the client is gone or the app quits.
    fn wait(&mut self, total: Duration) -> bool {
        let end = Instant::now() + total;
        loop {
            if QUITTING.load(Ordering::Relaxed) {
                return false;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.states.recv_timeout(left.min(READ_TIMEOUT)) {
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

    /// Pulls every pending state; false when the client is gone or the app
    /// quits.
    fn drain(&mut self) -> bool {
        if QUITTING.load(Ordering::Relaxed) {
            return false;
        }
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
        if !self.check_beacon() {
            return Outcome::Stopped;
        }
        if self.hosted.is_none() {
            let ntfy = self.config.relay_url.clone();
            match announce::find(&ntfy, &self.config.key, &self.config.room) {
                // ntfy refusing (its rate limit) or out of reach says nothing
                // about a host, and one that can host still serves Studio on
                // this computer from the loopback port meanwhile. Should a
                // host elsewhere turn out to be up, the newer one wins
                // (see host_check) once this one gets its announcement out.
                Err(why) if self.config.hosting.is_none() => return Outcome::Failed(why),
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
                _ => {
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

    /// Starts the room's relay and sends it off to go public. The session
    /// joins it on the loopback port meanwhile, so Studio on this computer
    /// sees this editor at once, not after the tunnel and its announcement.
    fn start_hosting(&mut self, hosting: &Hosting) -> Result<(), String> {
        let host = self.start_host().map_err(|e| e.to_string())?;
        let going = Going::start(hosting, host.port(), host.id().to_owned(), self.topic());
        self.hosted = Some(Hosted {
            host,
            going: Some(going),
            public: None,
            retry_at: None,
            retry_wait: TUNNEL_RETRY_START,
        });
        Ok(())
    }

    /// Takes the outcome of going public once there is one, and goes public
    /// again once a failed try's wait is over. A failure keeps the relay on
    /// the loopback port, so Studio on this computer stays connected.
    fn check_going(&mut self) {
        let topic = self.topic();
        let Some(hosted) = self.hosted.as_mut() else {
            return;
        };
        if hosted.going.is_none() {
            if hosted.public.is_none() && hosted.retry_at.is_some_and(|at| Instant::now() >= at) {
                hosted.retry_at = None;
                if let Some(hosting) = &self.config.hosting {
                    let id = hosted.host.id().to_owned();
                    hosted.going = Some(Going::start(hosting, hosted.host.port(), id, topic));
                }
            }
            return;
        }
        let Some(went) = hosted.going.as_ref().and_then(Going::poll) else {
            return;
        };
        hosted.going = None;
        match went {
            Ok((url, tunnel)) => {
                let _ = self.events.send(Event::Route(Route::Hosting(url.clone())));
                hosted.retry_wait = TUNNEL_RETRY_START;
                hosted.public = Some(Announced {
                    tunnel,
                    url,
                    at: unix_ms(),
                });
            }
            Err(refused) => self.tunnel_failed(refused),
        }
    }

    /// Schedules the next try at going public: when the tunnel service
    /// said (a rate limit), else after a wait that doubles on every failure
    /// in a row, so a tunnel that won't open isn't asked for in a loop.
    fn tunnel_failed(&mut self, refused: TunnelRefused) {
        let Some(hosted) = self.hosted.as_mut() else {
            return;
        };
        hosted.public = None;
        let wait = refused.retry_in.unwrap_or(hosted.retry_wait);
        hosted.retry_wait = (hosted.retry_wait * 2).min(TUNNEL_RETRY_MAX);
        hosted.retry_at = Some(Instant::now() + wait);
        let _ = self.events.send(Event::RemotePaused(Paused {
            why: refused.why,
            rate_limited: refused.retry_in.is_some(),
            retry_at_ms: unix_ms() + wait.as_millis() as u64,
        }));
    }

    fn topic(&self) -> Topic {
        Topic {
            ntfy: self.config.relay_url.clone(),
            key: self.config.key.clone(),
            room: self.config.room.clone(),
        }
    }

    /// The relay, on the room's loopback port so Studio on this computer
    /// finds it without the tunnel. A presence restart can leave the worker
    /// it replaced still holding that port for a moment, so it is asked for
    /// a few times before any free port does.
    fn start_host(&mut self) -> std::io::Result<host::Host> {
        let port = announce::loopback_port(&self.config.room);
        for _ in 0..LOOPBACK_TRIES {
            let id = announce::host_id(&self.config.beacon_from);
            if let Ok(host) = host::Host::start_on(&self.config.key, port, &id) {
                return Ok(host);
            }
            if !self.wait(Duration::from_millis(500)) {
                return Err(std::io::Error::other("presence stopped"));
            }
        }
        host::Host::start(&self.config.key)
    }

    /// Posts this client's beacon when due and reads the others' at most once
    /// a [`ROUTE_CHECK`], reporting a change in what it finds. Failures are
    /// ignored: the beacon only explains. False when the client is gone.
    fn check_beacon(&mut self) -> bool {
        let due = |at: Option<Instant>, every: Duration| at.is_none_or(|t| t.elapsed() >= every);
        let ntfy = self.config.relay_url.clone();
        let fp = crate::beacon::fingerprint(&self.config.room);
        let (place, from) = (self.config.place_id, self.config.beacon_from.clone());
        if due(
            self.beacon_posted,
            Duration::from_millis(crate::beacon::EVERY_MS),
        ) {
            self.beacon_posted = Some(Instant::now());
            let _ = crate::beacon::publish(&ntfy, place, &fp, &from);
        }
        if !due(self.beacon_read, ROUTE_CHECK) {
            return true;
        }
        self.beacon_read = Some(Instant::now());
        let Ok(found) = crate::beacon::read(&ntfy, place, &fp, &from) else {
            return true;
        };
        if found == self.key_conflict {
            return true;
        }
        self.key_conflict = found.clone();
        self.events.send(Event::KeyConflict(found)).is_ok()
    }

    /// The client is gone: stops hosting and takes this client's beacon down.
    fn leave(&mut self) {
        self.stop_hosting();
        if self.config.kind == RelayKind::Hosted && self.beacon_posted.is_some() {
            let config = &self.config;
            let _ =
                crate::beacon::publish(&config.relay_url, config.place_id, "", &config.beacon_from);
        }
    }

    /// Leaves the room's hosting: says goodbye so guests look again at once,
    /// then closes the tunnel and the relay. Quitting skips checking for a
    /// newer host first, one request fewer before the app exits.
    fn stop_hosting(&mut self) {
        if let Some(Hosted {
            public: Some(public),
            ..
        }) = self.hosted.take()
        {
            going::goodbye(
                &self.topic(),
                &public.url,
                !QUITTING.load(Ordering::Relaxed),
            );
        }
    }

    /// The periodic hosted-room check; `Some` ends the session (to rejoin or
    /// re-host): a host whose tunnel died or that a newer host replaced, or a
    /// guest whose host moved or left.
    fn check_route(&mut self) -> Option<Outcome> {
        if !self.check_beacon() {
            return Some(Outcome::Stopped);
        }
        let found = announce::find(&self.config.relay_url, &self.config.key, &self.config.room);
        if let Some(hosted) = &mut self.hosted {
            let Some(public) = &mut hosted.public else {
                // Only on loopback, waiting to try the tunnel again: a host
                // elsewhere that is public serves everyone, so join it.
                let local = hosted.host.local_url();
                return match &found {
                    Ok(Some(newest))
                        if hosted.going.is_none()
                            && newest.url != local
                            && reachable(&newest.url) =>
                    {
                        self.hosted = None;
                        Some(Outcome::Failed(
                            "another rbx-native user hosts the room".into(),
                        ))
                    }
                    _ => None,
                };
            };
            if !public.tunnel.alive() {
                self.tunnel_failed("the tunnel closed".into());
                return None;
            }
            let check = match &found {
                Ok(newest) => host_check(&public.url, public.at, newest.as_ref(), reachable),
                Err(_) => HostCheck::Keep,
            };
            if check == HostCheck::Yield {
                self.hosted = None;
                return Some(Outcome::Failed(
                    "another rbx-native user hosts the room".into(),
                ));
            }
            let now = unix_ms();
            let due = now.saturating_sub(public.at) >= announce::REANNOUNCE_MS;
            if (due || check == HostCheck::Reannounce)
                && announce::publish(
                    &self.config.relay_url,
                    &self.config.key,
                    &self.config.room,
                    &public.url,
                    hosted.host.id(),
                )
                .is_ok()
            {
                public.at = now;
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
            self.check_going();
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
                    if let Err(error) = crate::tick::send(&mut socket, Message::text(json)) {
                        return Outcome::Failed(error.to_string());
                    }
                    last_sent = Instant::now();
                }
                self.dirty = false;
            }
            if let Err(error) = crate::tick::flush(&mut socket) {
                return Outcome::Failed(error.to_string());
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
                Err(error) if crate::tick::nothing_read(&error, READ_TIMEOUT) => {}
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
        let agent: ureq::Agent = rbx_net::config()
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
        let config = &self.config;
        connect_route(
            relay_url,
            &config.room,
            &config.key,
            config.user_id,
            "ws",
            READ_TIMEOUT,
        )
    }
}

/// What a host does about the room's newest announcement.
#[derive(Debug, PartialEq, Eq)]
enum HostCheck {
    /// It is this host's own.
    Keep,
    /// Another host announced since and answers: two hosted at once, and the
    /// newer one keeps the room.
    Yield,
    /// A goodbye, or an address nobody answers on (a host that crashed or
    /// quit, this editor's own earlier tunnel): announce again, so
    /// collaborators who went there come back here.
    Reannounce,
}

/// [`HostCheck`] for a host public at `public` since `announced`, given the
/// newest genuine announcement (`None`: none, or a goodbye). `live` is only
/// asked about a newer foreign address, so a stale or self-made one never
/// takes the room away from a host that still runs.
fn host_check(
    public: &str,
    announced: u64,
    newest: Option<&announce::Announcement>,
    live: impl FnOnce(&str) -> bool,
) -> HostCheck {
    match newest {
        Some(newest) if newest.url == public => HostCheck::Keep,
        Some(newest) if newest.at > announced && live(&newest.url) => HostCheck::Yield,
        _ => HostCheck::Reannounce,
    }
}

/// How many times [`Worker::start_host`] asks for the loopback port.
const LOOPBACK_TRIES: u32 = 6;

/// Whether a relay answers its health check (a host that crashed leaves its
/// last announcement behind).
fn reachable(url: &str) -> bool {
    let agent: ureq::Agent = rbx_net::config()
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
    use tungstenite::handshake::server::{ErrorResponse, Request, Response};

    #[test]
    fn a_host_yields_only_to_a_newer_host_that_answers() {
        const OURS: &str = "https://ours.trycloudflare.com";
        let at = |url: &str, at: u64| announce::Announcement {
            v: 1,
            url: url.into(),
            at,
            sig: String::new(),
            host: String::new(),
        };
        let never = |_: &str| -> bool { panic!("asked about our own address") };
        assert_eq!(
            host_check(OURS, 100, Some(&at(OURS, 99)), never),
            HostCheck::Keep
        );
        let newer = at("https://other.trycloudflare.com", 200);
        assert_eq!(
            host_check(OURS, 100, Some(&newer), |_| true),
            HostCheck::Yield
        );
        // A newer address nobody answers on (a crashed host, or this
        // editor's own earlier tunnel) takes nothing away: announce again.
        assert_eq!(
            host_check(OURS, 100, Some(&newer), |_| false),
            HostCheck::Reannounce
        );
        let older = at("https://other.trycloudflare.com", 50);
        assert_eq!(
            host_check(OURS, 100, Some(&older), |_| true),
            HostCheck::Reannounce
        );
        // A goodbye after ours would leave collaborators waiting on a host
        // that is still here.
        assert_eq!(host_check(OURS, 100, None, never), HostCheck::Reannounce);
    }

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
        let listener = crate::announce::bind_outside_loopback_range().unwrap();
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
            place_id: 4567,
            beacon_from: "test".into(),
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
