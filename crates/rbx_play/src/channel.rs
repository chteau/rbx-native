//! Loopback HTTP/1.1 server the probe reaches through the tunnel. Plain
//! `std::net`, one thread per connection, `Connection: close` on every reply.
//! Routes and status codes are documented in `crate::protocol`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::protocol::{Command, CommandBody, Token, Uplink, TOKEN_HEADER};

/// How long `/v1/poll` holds a request open before answering `204`.
pub const DEFAULT_HOLD: Duration = Duration::from_secs(25);
/// Largest request body accepted; bigger ones get `413`.
pub const MAX_BODY: usize = 1 << 20;

const MAX_HEAD: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Queue {
    last_id: u64,
    /// Bumped on every token rotation so held polls know their session ended.
    epoch: u64,
    commands: Vec<Command>,
}

struct Shared {
    token: Mutex<Token>,
    hold: Duration,
    queue: Mutex<Queue>,
    wake: Condvar,
    shutdown: AtomicBool,
    events: mpsc::Sender<Uplink>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct ChannelServer {
    shared: Arc<Shared>,
    addr: SocketAddr,
    events: mpsc::Receiver<Uplink>,
}

impl ChannelServer {
    /// Binds `127.0.0.1:0` and starts accepting on a background thread.
    pub fn start(token: Token, hold: Duration) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            token: Mutex::new(token),
            hold,
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
            events: tx,
        });
        let accept_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("channel-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accept_shared.shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let conn_shared = Arc::clone(&accept_shared);
                    let _ = std::thread::Builder::new()
                        .name("channel-conn".into())
                        .spawn(move || {
                            let _ = handle(&conn_shared, stream);
                        });
                }
            })?;
        Ok(ChannelServer {
            shared,
            addr,
            events: rx,
        })
    }

    /// The bound loopback port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Starts a new Play session on the same port (so the tunnel stays up):
    /// requests must now carry `token`, queued commands are dropped, and
    /// held polls answer `204`. Command ids keep increasing across sessions.
    pub fn rotate_token(&self, token: Token) {
        *self.shared.token.lock().unwrap_or_else(|e| e.into_inner()) = token;
        let mut q = self.shared.lock();
        q.commands.clear();
        q.epoch += 1;
        drop(q);
        self.shared.wake.notify_all();
    }

    /// Queues a command for the probe; returns its id (1, 2, 3, …).
    /// Wakes any held `/v1/poll` whose `since` is below it.
    pub fn send(&self, body: CommandBody) -> u64 {
        let mut q = self.shared.lock();
        q.last_id += 1;
        let id = q.last_id;
        q.commands.push(Command { id, body });
        drop(q);
        self.shared.wake.notify_all();
        id
    }

    /// Everything the probe has posted, in arrival order. An `Ack` also
    /// removes its command from the queue so it is not re-sent.
    pub fn events(&self) -> &mpsc::Receiver<Uplink> {
        &self.events
    }
}

impl Drop for ChannelServer {
    /// Stops accepting; held polls answer `204` and the thread exits.
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        // Take the lock so a waiter cannot miss the flag between check and wait.
        drop(self.shared.lock());
        self.shared.wake.notify_all();
        let _ = TcpStream::connect(self.addr);
    }
}

struct Request {
    method: String,
    target: String,
    token: Option<String>,
    content_length: usize,
}

enum Head {
    Ok(Request),
    Bad,
}

fn read_head(reader: &mut BufReader<TcpStream>) -> std::io::Result<Head> {
    let mut total = 0;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    total += line.len();
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(Head::Bad);
    };
    let mut req = Request {
        method: method.to_owned(),
        target: target.to_owned(),
        token: None,
        content_length: 0,
    };
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        total += n;
        if n == 0 || total > MAX_HEAD {
            return Ok(Head::Bad);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            return Ok(Head::Ok(req));
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            return Ok(Head::Bad);
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            match value.parse() {
                Ok(n) => req.content_length = n,
                Err(_) => return Ok(Head::Bad),
            }
        } else if name.eq_ignore_ascii_case(TOKEN_HEADER) {
            req.token = Some(value.to_owned());
        }
    }
}

fn reply(stream: &mut TcpStream, status: u16, body: &str, json: bool) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let ctype = if json {
        "application/json"
    } else {
        "text/plain"
    };
    let msg = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(msg.as_bytes())?;
    stream.flush()
}

fn handle(shared: &Shared, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let req = match read_head(&mut reader)? {
        Head::Ok(r) => r,
        Head::Bad => return finish(&mut stream, &mut reader, 400, ""),
    };
    let authorised = req.token.as_deref().is_some_and(|t| {
        shared
            .token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .matches(t)
    });
    if !authorised {
        return finish(&mut stream, &mut reader, 404, "");
    }
    let (path, query) = req.target.split_once('?').unwrap_or((&req.target, ""));
    match (req.method.as_str(), path) {
        ("GET", "/healthz") => finish(&mut stream, &mut reader, 200, "ok"),
        ("POST", "/v1/ingest") => {
            if req.content_length > MAX_BODY {
                return finish(&mut stream, &mut reader, 413, "");
            }
            let mut body = vec![0u8; req.content_length];
            reader.read_exact(&mut body)?;
            if trace() {
                eprintln!(
                    "rbx_play ingest: {}",
                    String::from_utf8_lossy(&body[..body.len().min(600)])
                );
            }
            match serde_json::from_slice::<Vec<Uplink>>(&body) {
                Ok(batch) => {
                    for up in batch {
                        if let Uplink::Ack { id, .. } = &up {
                            shared.lock().commands.retain(|c| c.id != *id);
                        }
                        let _ = shared.events.send(up);
                    }
                    finish(&mut stream, &mut reader, 204, "")
                }
                Err(_) => finish(&mut stream, &mut reader, 400, ""),
            }
        }
        ("GET", "/v1/poll") => {
            let since = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("since="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let pending = wait_for_commands(shared, since);
            if pending.is_empty() {
                finish(&mut stream, &mut reader, 204, "")
            } else {
                let json = serde_json::to_string(&pending).unwrap_or_else(|_| "[]".into());
                if trace() {
                    eprintln!("rbx_play poll -> {}", &json[..json.len().min(600)]);
                }
                reply(&mut stream, 200, &json, true)?;
                let _ = stream.shutdown(Shutdown::Write);
                Ok(())
            }
        }
        _ => finish(&mut stream, &mut reader, 404, ""),
    }
}

fn wait_for_commands(shared: &Shared, since: u64) -> Vec<Command> {
    let deadline = Instant::now() + shared.hold;
    let mut q = shared.lock();
    let epoch = q.epoch;
    loop {
        if q.epoch != epoch {
            return Vec::new();
        }
        let pending: Vec<Command> = q
            .commands
            .iter()
            .filter(|c| c.id > since)
            .cloned()
            .collect();
        if !pending.is_empty() {
            return pending;
        }
        let now = Instant::now();
        if shared.shutdown.load(Ordering::SeqCst) || now >= deadline {
            return Vec::new();
        }
        q = shared
            .wake
            .wait_timeout(q, deadline - now)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// Replies, then drains unread request bytes briefly so closing the socket
/// does not reset the connection before the client has read the reply.
fn finish(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    reply(stream, status, body, false)?;
    let _ = stream.shutdown(Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let mut sink = [0u8; 8192];
    let mut drained = 0usize;
    while drained < 2 * MAX_BODY {
        match reader.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
    Ok(())
}

/// `RBX_PLAY_TRACE=1` prints every ingested batch and every command handed
/// to the probe on stderr (truncated), for diagnosing a Play session.
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RBX_PLAY_TRACE").is_some_and(|v| v != "0"))
}
