//! A local stand-in for the Open Cloud v2 data store, ordered data store and
//! memory store endpoints, so the browser UI can be exercised without a key or
//! a real universe. It serves a deterministic fake universe on a loopback port.

mod fixture;
mod other;
mod standard;
#[cfg(test)]
mod tests;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use serde_json::{json, Map, Value};

use fixture::State;

pub const MOCK_UNIVERSE_ID: u64 = 4_200_000_001;

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct MockOptions {
    /// Scopes the fake key lacks; routes needing one answer with the gateway's 403.
    pub denied_scopes: Vec<String>,
    /// Every n-th request is answered with 429 instead of being served.
    pub throttle_every: Option<u32>,
}

type Reply = (u16, Value);

#[derive(Clone, Copy)]
enum Kind {
    ListStores,
    ListEntries,
    CreateEntry,
    GetEntry,
    UpdateEntry,
    Increment,
    DeleteEntry,
    ListRevisions,
    OrderedList,
    OrderedGet,
    OrderedCreate,
    OrderedUpdate,
    OrderedIncrement,
    OrderedDelete,
    MapList,
    MapGet,
    MapCreate,
    MapUpdate,
    MapDelete,
    QueueAdd,
    QueueRead,
    QueueDiscard,
}

/// `store` doubles as the sorted map or queue name for the memory store kinds.
struct Route {
    kind: Kind,
    store: String,
    scope: String,
    key: String,
}

struct Request {
    universe: String,
    query: Vec<(String, String)>,
    body: Value,
}

impl Request {
    fn q(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Shared {
    state: Mutex<State>,
    options: MockOptions,
    requests: AtomicU64,
}

/// Starts the mock on a free loopback port and returns its base URL.
pub fn start(options: MockOptions) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback port is available");
    let addr = listener
        .local_addr()
        .expect("a bound socket has an address");
    let shared = Arc::new(Shared {
        state: Mutex::new(State::new()),
        options,
        requests: AtomicU64::new(0),
    });
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = Arc::clone(&shared);
            thread::spawn(move || serve(stream, &shared));
        }
    });
    format!("http://{addr}")
}

fn ok(value: Value) -> Reply {
    (200, value)
}

fn fail(status: u16, code: i64, message: &str) -> Reply {
    (status, json!({"code": code, "message": message}))
}

fn listing(field: &str, items: Vec<Value>, next: Option<String>) -> Reply {
    let mut map = Map::new();
    map.insert(field.to_owned(), Value::Array(items));
    if let Some(token) = next {
        map.insert("nextPageToken".to_owned(), Value::String(token));
    }
    ok(Value::Object(map))
}

/// Page tokens are plain offsets; the mock never reorders between pages.
fn page<T: Clone>(items: &[T], req: &Request, default: usize) -> (Vec<T>, Option<String>) {
    let size = req
        .q("maxPageSize")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .map_or(default, |n| n.min(default));
    let start = req
        .q("pageToken")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(items.len());
    let end = (start + size).min(items.len());
    let next = (end < items.len()).then(|| end.to_string());
    (items[start..end].to_vec(), next)
}

fn starts_with(req: &Request) -> String {
    req.q("filter")
        .and_then(|f| {
            let open = f.find('"')?;
            let close = f.rfind('"')?;
            (close > open).then(|| f[open + 1..close].to_owned())
        })
        .unwrap_or_default()
}

/// `(operator, operand)` pairs from filters like `entry >= 10 && entry <= 30`.
fn conditions(req: &Request) -> Vec<(String, String)> {
    req.q("filter")
        .map(|f| {
            f.split("&&")
                .filter_map(|part| {
                    let mut tokens = part.split_whitespace();
                    tokens.next()?;
                    let op = tokens.next()?.to_owned();
                    let operand = tokens.collect::<Vec<_>>().join(" ");
                    Some((op, operand.trim_matches('"').to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn holds<T: PartialOrd>(left: T, op: &str, right: T) -> bool {
    match op {
        ">=" => left >= right,
        "<=" => left <= right,
        ">" => left > right,
        "<" => left < right,
        "==" | "=" => left == right,
        _ => true,
    }
}

fn route(kind: Kind, store: &str, scope: &str, key: &str) -> Route {
    Route {
        kind,
        store: store.to_owned(),
        scope: scope.to_owned(),
        key: key.to_owned(),
    }
}

/// Maps a request to its route and the API key scope the real service demands.
fn parse(method: &str, rest: &[&str]) -> Option<(Route, &'static str)> {
    use Kind::*;
    const ORDERED_READ: &str = "universe.ordered-data-store.scope.entry:read";
    const ORDERED_WRITE: &str = "universe.ordered-data-store.scope.entry:write";
    const MAP_READ: &str = "memory-store.sorted-map:read";
    const MAP_WRITE: &str = "memory-store.sorted-map:write";
    Some(match (method, rest) {
        ("GET", ["data-stores"]) => (
            route(ListStores, "", "", ""),
            "universe-datastores.control:list",
        ),
        ("GET", ["data-stores", ds, "scopes", sc, "entries"]) => (
            route(ListEntries, ds, sc, ""),
            "universe-datastores.objects:list",
        ),
        ("POST", ["data-stores", ds, "scopes", sc, "entries"]) => (
            route(CreateEntry, ds, sc, ""),
            "universe-datastores.objects:create",
        ),
        (m, ["data-stores", ds, "scopes", sc, "entries", k]) => {
            if let Some(key) = k.strip_suffix(":increment") {
                (m == "POST").then_some(())?;
                (
                    route(Increment, ds, sc, key),
                    "universe-datastores.objects:update",
                )
            } else if let Some(key) = k.strip_suffix(":listRevisions") {
                (m == "GET").then_some(())?;
                (
                    route(ListRevisions, ds, sc, key),
                    "universe-datastores.versions:list",
                )
            } else {
                match m {
                    "GET" => (
                        route(GetEntry, ds, sc, k),
                        "universe-datastores.objects:read",
                    ),
                    "PATCH" => (
                        route(UpdateEntry, ds, sc, k),
                        "universe-datastores.objects:update",
                    ),
                    "DELETE" => (
                        route(DeleteEntry, ds, sc, k),
                        "universe-datastores.objects:delete",
                    ),
                    _ => return None,
                }
            }
        }
        ("GET", ["ordered-data-stores", ds, "scopes", sc, "entries"]) => {
            (route(OrderedList, ds, sc, ""), ORDERED_READ)
        }
        ("POST", ["ordered-data-stores", ds, "scopes", sc, "entries"]) => {
            (route(OrderedCreate, ds, sc, ""), ORDERED_WRITE)
        }
        (m, ["ordered-data-stores", ds, "scopes", sc, "entries", k]) => {
            if let Some(key) = k.strip_suffix(":increment") {
                (m == "POST").then_some(())?;
                (route(OrderedIncrement, ds, sc, key), ORDERED_WRITE)
            } else {
                match m {
                    "GET" => (route(OrderedGet, ds, sc, k), ORDERED_READ),
                    "PATCH" => (route(OrderedUpdate, ds, sc, k), ORDERED_WRITE),
                    "DELETE" => (route(OrderedDelete, ds, sc, k), ORDERED_WRITE),
                    _ => return None,
                }
            }
        }
        ("GET", ["memory-store", "sorted-maps", map, "items"]) => {
            (route(MapList, map, "", ""), MAP_READ)
        }
        ("POST", ["memory-store", "sorted-maps", map, "items"]) => {
            (route(MapCreate, map, "", ""), MAP_WRITE)
        }
        (m, ["memory-store", "sorted-maps", map, "items", k]) => match m {
            "GET" => (route(MapGet, map, "", k), MAP_READ),
            "PATCH" => (route(MapUpdate, map, "", k), MAP_WRITE),
            "DELETE" => (route(MapDelete, map, "", k), MAP_WRITE),
            _ => return None,
        },
        ("POST", ["memory-store", "queues", q, "items"]) => {
            (route(QueueAdd, q, "", ""), "memory-store.queue:add")
        }
        ("GET", ["memory-store", "queues", q, "items:read"]) => {
            (route(QueueRead, q, "", ""), "memory-store.queue:dequeue")
        }
        ("POST", ["memory-store", "queues", q, "items:discard"]) => {
            (route(QueueDiscard, q, "", ""), "memory-store.queue:discard")
        }
        _ => return None,
    })
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(shared: &Shared, method: &str, target: &str, body: &[u8]) -> (Reply, bool) {
    let n = shared.requests.fetch_add(1, Ordering::SeqCst) + 1;
    if shared
        .options
        .throttle_every
        .is_some_and(|every| every > 0 && n.is_multiple_of(u64::from(every)))
    {
        return (
            (
                429,
                json!({"code": "RESOURCE_EXHAUSTED", "message": "Too many requests."}),
            ),
            true,
        );
    }
    let missing = || (fail(404, 5, "Not found."), false);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let segments: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(decode)
        .collect();
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    let ["cloud", "v2", "universes", universe, rest @ ..] = segments.as_slice() else {
        return missing();
    };
    let Some((route, scope)) = parse(method, rest) else {
        return missing();
    };
    if shared.options.denied_scopes.iter().any(|s| s == scope) {
        return (
            (
                403,
                json!({"errors": [{"code": 0, "message": "Scope not authorized."}]}),
            ),
            false,
        );
    }
    let request = Request {
        universe: (*universe).to_owned(),
        query: query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (decode(k), decode(v))
            })
            .collect(),
        body: serde_json::from_slice(body).unwrap_or(Value::Null),
    };
    let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
    let reply = standard::handle(&mut state, &request, &route)
        .or_else(|| other::handle(&mut state, &request, &route))
        .unwrap_or_else(|| fail(404, 5, "Not found."));
    (reply, false)
}

fn read_request(stream: &TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
        }
    }
    if length > MAX_BODY_BYTES {
        return None;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some((method, target, body))
}

fn serve(mut stream: TcpStream, shared: &Shared) {
    let Some((method, target, body)) = read_request(&stream) else {
        return;
    };
    let ((status, payload), retry) = respond(shared, &method, &target, &body);
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let payload = payload.to_string();
    let retry_header = if retry { "retry-after: 1\r\n" } else { "" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{retry_header}connection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(payload.as_bytes());
    let _ = stream.flush();
}
