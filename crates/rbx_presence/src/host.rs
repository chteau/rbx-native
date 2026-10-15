//! The relay, run by rbx-native itself: the same routes, signatures and
//! limits as the Cloudflare Worker in `relay/` (`/v1/rooms/<room>/state`,
//! `poll`, `journal` and `ws`), for one room, on a loopback port that a
//! Cloudflare quick tunnel makes public. Nobody needs an account or a
//! server: whoever opens the place in rbx-native first hosts it, and
//! [`crate::announce`] tells everyone else where.
//!
//! Plain `std::net`, one thread per connection, `Connection: close` on every
//! HTTP reply. The room lives in memory only and is gone when the host
//! stops; payloads are never logged.

mod room;
mod validate;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

use crate::auth;
use crate::schema::{Client, MAX_BODY_BYTES};
pub use room::{Refusal, Room};
pub use validate::parse_state;

/// A long-poll answers after this long even when nothing changed.
pub const POLL_TIMEOUT: Duration = Duration::from_secs(20);
/// How often silent senders are swept out.
const SWEEP: Duration = Duration::from_secs(5);
/// How often a socket thread checks for a newer snapshot to push.
const SOCKET_TICK: Duration = Duration::from_millis(100);
const MAX_HEAD: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

struct Shared {
    key: String,
    room_id: String,
    room: Mutex<Room>,
    changed: Condvar,
    stop: AtomicBool,
    /// Live sockets per sender, so the last one closing removes the state.
    sockets: Mutex<Vec<(u64, Client)>>,
    /// Every open socket's outbox: each change is pushed to all of them,
    /// as the Worker does, so a state that comes and goes between two reads
    /// still reaches everyone.
    outboxes: Mutex<Vec<(u64, Sender<String>)>>,
    next_outbox: AtomicU64,
}

impl Shared {
    fn room(&self) -> MutexGuard<'_, Room> {
        self.room.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The room changed: wake long-polls and push the snapshot to sockets.
    fn changed(&self) {
        let text = serde_json::to_string(&self.room().snapshot()).unwrap_or_default();
        self.changed.notify_all();
        lock(&self.outboxes).retain(|(_, outbox)| outbox.send(text.clone()).is_ok());
    }

    fn subscribe(&self) -> (u64, Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        let id = self.next_outbox.fetch_add(1, Ordering::SeqCst);
        lock(&self.outboxes).push((id, tx));
        (id, rx)
    }

    fn unsubscribe(&self, id: u64) {
        lock(&self.outboxes).retain(|(other, _)| *other != id);
    }
}

/// A running host; dropping it stops it.
pub struct Host {
    shared: Arc<Shared>,
    addr: SocketAddr,
}

impl Host {
    /// Binds `127.0.0.1:0` for the room of `key` and starts serving.
    pub fn start(key: &str) -> std::io::Result<Host> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            key: key.to_owned(),
            room_id: auth::room_id(key),
            room: Mutex::new(Room::default()),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
            sockets: Mutex::new(Vec::new()),
            outboxes: Mutex::new(Vec::new()),
            next_outbox: AtomicU64::new(0),
        });

        let accept = Arc::clone(&shared);
        thread::Builder::new()
            .name("presence-host".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accept.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let conn = Arc::clone(&accept);
                    let _ = thread::Builder::new()
                        .name("presence-host-conn".into())
                        .spawn(move || handle(&conn, stream));
                }
            })?;

        let sweep = Arc::clone(&shared);
        thread::Builder::new()
            .name("presence-host-sweep".into())
            .spawn(move || {
                while !sweep.stop.load(Ordering::SeqCst) {
                    thread::sleep(SWEEP);
                    let expired = sweep.room().expire(crate::now_ms());
                    if expired {
                        sweep.changed();
                    }
                }
            })?;
        Ok(Host { shared, addr })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// `http://127.0.0.1:<port>`, for this machine's own client.
    pub fn local_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.changed.notify_all();
        // Unblocks `incoming()`.
        let _ = TcpStream::connect(self.addr);
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    fn query(&self, name: &str) -> Option<&str> {
        let query = self.target.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k == name).then_some(v)
        })
    }
}

enum Incoming {
    Request(Request),
    TooLarge,
    Bad,
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Incoming {
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return Incoming::Bad;
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Incoming::Bad;
    };
    let (method, target) = (method.to_owned(), target.to_owned());
    let mut headers = Vec::new();
    let mut head = line.len();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return Incoming::Bad;
        }
        head += line.len();
        if head > MAX_HEAD {
            return Incoming::TooLarge;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let mut request = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if request
        .header("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        loop {
            let mut size = String::new();
            if reader.read_line(&mut size).unwrap_or(0) == 0 {
                return Incoming::Bad;
            }
            let Ok(n) = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
            else {
                return Incoming::Bad;
            };
            if request.body.len() + n > MAX_BODY_BYTES {
                return Incoming::TooLarge;
            }
            let mut chunk = vec![0; n + 2];
            if reader.read_exact(&mut chunk).is_err() {
                return Incoming::Bad;
            }
            if n == 0 {
                break;
            }
            request.body.extend_from_slice(&chunk[..n]);
        }
    } else if let Some(len) = request.header("content-length") {
        let Ok(len) = len.parse::<usize>() else {
            return Incoming::Bad;
        };
        if len > MAX_BODY_BYTES {
            return Incoming::TooLarge;
        }
        request.body = vec![0; len];
        if reader.read_exact(&mut request.body).is_err() {
            return Incoming::Bad;
        }
    }
    Incoming::Request(request)
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn refuse(stream: &mut TcpStream, refusal: Refusal) {
    respond(
        stream,
        refusal.status,
        &serde_json::json!({ "error": refusal.error }).to_string(),
    );
}

fn handle(shared: &Arc<Shared>, stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut stream = stream;
    let request = match read_request(&mut reader) {
        Incoming::Request(request) => request,
        Incoming::TooLarge => return refuse(&mut stream, Refusal::new(413, "size")),
        Incoming::Bad => return,
    };

    if request.method == "GET" && request.path() == "/health" {
        return respond(&mut stream, 200, "\"ok\"");
    }
    // `/v1/rooms/<room>/<route>`; this host serves its one room only.
    let route = request
        .path()
        .strip_prefix("/v1/rooms/")
        .and_then(|rest| rest.split_once('/'))
        .filter(|(room, _)| *room == shared.room_id)
        .map(|(_, route)| route.to_owned());
    let Some(route) = route else {
        return respond(&mut stream, 404, "\"not found\"");
    };
    let method = request.method.as_str();
    let allowed = matches!(
        (route.as_str(), method),
        ("state", "POST" | "DELETE") | ("poll" | "journal" | "ws", "GET")
    );
    if !allowed {
        return respond(&mut stream, 405, "\"method not allowed\"");
    }

    let user_id = request
        .header(auth::USER_HEADER)
        .and_then(|v| v.parse::<u64>().ok());
    let ts = request
        .header(auth::TIME_HEADER)
        .and_then(|v| v.parse::<u64>().ok());
    let sig = request.header(auth::SIG_HEADER);
    let (Some(user_id), Some(ts), Some(sig)) = (user_id, ts, sig) else {
        return refuse(&mut stream, Refusal::new(401, "sig"));
    };
    // A key offered with the request must be this room's (the Worker learns
    // its key this way; the host already has it).
    if let Some(offered) = request.header(auth::KEY_HEADER) {
        if auth::room_id(offered) != shared.room_id {
            return refuse(&mut stream, Refusal::new(401, "sig"));
        }
    }
    let now = crate::now_ms();
    if let Err(fault) = shared.room().check_time(user_id, ts, now, false) {
        return refuse(&mut stream, fault);
    }
    let canonical = auth::canonical(method, &request.target, user_id, ts, &request.body);
    if !constant_eq(&auth::sign(&shared.key, &canonical), sig) {
        return refuse(&mut stream, Refusal::new(401, "sig"));
    }
    if method != "GET" {
        // After the signature, so an unsigned request cannot advance it.
        if let Err(fault) = shared.room().check_time(user_id, ts, now, true) {
            return refuse(&mut stream, fault);
        }
    }

    match (route.as_str(), method) {
        ("state", "POST") => {
            let body = String::from_utf8_lossy(&request.body);
            let result =
                parse_state(&body, user_id).and_then(|state| shared.room().put(state, now));
            match result {
                Ok(()) => {
                    shared.changed();
                    let cursor = shared.room().cursor;
                    respond(
                        &mut stream,
                        200,
                        &serde_json::json!({ "cursor": cursor }).to_string(),
                    );
                }
                Err(fault) => refuse(&mut stream, fault),
            }
        }
        ("state", _) => {
            // Only the plugin deletes over HTTP; `?client=` names the editor.
            let client = if request.query("client") == Some("rbx-native") {
                Client::RbxNative
            } else {
                Client::Studio
            };
            let removed = shared.room().remove(user_id, client);
            if removed {
                shared.changed();
            }
            let cursor = shared.room().cursor;
            respond(
                &mut stream,
                200,
                &serde_json::json!({ "cursor": cursor }).to_string(),
            );
        }
        ("journal", _) => {
            let Some(since) = number_query(&request, "since") else {
                return refuse(&mut stream, Refusal::new(400, "schema"));
            };
            let entries = shared.room().journal_since(since, now);
            respond(
                &mut stream,
                200,
                &serde_json::json!({ "entries": entries }).to_string(),
            );
        }
        ("poll", _) => {
            let Some(since) = number_query(&request, "since") else {
                return refuse(&mut stream, Refusal::new(400, "schema"));
            };
            let deadline = Instant::now() + POLL_TIMEOUT;
            let mut room = shared.room();
            while room.cursor <= since && !shared.stop.load(Ordering::SeqCst) {
                let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                room = shared
                    .changed
                    .wait_timeout(room, left)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let body = serde_json::to_string(&room.snapshot()).unwrap_or_default();
            drop(room);
            respond(&mut stream, 200, &body);
        }
        _ => socket(shared, stream, &request, user_id),
    }
}

fn number_query(request: &Request, name: &str) -> Option<u64> {
    match request.query(name) {
        None => Some(0),
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64),
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// The WebSocket route: answers the upgrade itself, sends the snapshot,
/// takes states, and pushes every newer snapshot until either side leaves.
fn socket(shared: &Arc<Shared>, mut stream: TcpStream, request: &Request, user_id: u64) {
    let upgrade = request
        .header("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = request.header("sec-websocket-key").filter(|_| upgrade) else {
        return respond(&mut stream, 426, "\"expected websocket\"");
    };
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let _ = write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {accept}\r\n\r\n"
    );
    let _ = stream.set_read_timeout(Some(SOCKET_TICK));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);

    // Subscribe before taking the first snapshot, so no change between the
    // two is lost.
    let (outbox_id, outbox) = shared.subscribe();
    let first = serde_json::to_string(&shared.room().snapshot()).unwrap_or_default();
    let mut client: Option<Client> = None;
    if ws.send(Message::text(first)).is_ok() {
        loop {
            if shared.stop.load(Ordering::SeqCst) {
                let _ = ws.close(None);
                break;
            }
            let mut failed = false;
            while let Ok(text) = outbox.try_recv() {
                if ws.send(Message::text(text)).is_err() {
                    failed = true;
                    break;
                }
            }
            if failed {
                break;
            }
            match ws.read() {
                Ok(Message::Text(text)) => {
                    let result = parse_state(text.as_str(), user_id).and_then(|state| {
                        let editor = state.client;
                        shared.room().put(state, crate::now_ms()).map(|()| editor)
                    });
                    match result {
                        Ok(editor) => {
                            if client.is_none() {
                                client = Some(editor);
                                lock(&shared.sockets).push((user_id, editor));
                            }
                            shared.changed();
                        }
                        Err(_) => {
                            let _ = ws.close(None);
                            break;
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    }
    shared.unsubscribe(outbox_id);
    if let Some(editor) = client {
        let mut sockets = lock(&shared.sockets);
        if let Some(i) = sockets.iter().position(|s| *s == (user_id, editor)) {
            sockets.remove(i);
        }
        let another = sockets.contains(&(user_id, editor));
        drop(sockets);
        let removed = !another && shared.room().remove(user_id, editor);
        if removed {
            shared.changed();
        }
    }
    let _ = ws.get_mut().shutdown(Shutdown::Both);
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
