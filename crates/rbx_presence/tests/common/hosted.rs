//! A hosted room on this machine: a stand-in ntfy server and rbx-native
//! clients whose "tunnel" is the loopback port itself.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use rbx_presence::auth;
use rbx_presence::client::{Client, Config, Event, Hosting, Tunnel};
use rbx_presence::relay::RelayKind;
use rbx_presence::schema::{Client as Editor, UserState, SCHEMA_VERSION};

/// Each topic's messages, with when they were posted (unix seconds).
pub type Topics = Arc<Mutex<HashMap<String, Vec<(String, u64)>>>>;

/// `POST /<topic>` stores the body; `GET /<topic>/json` answers every stored
/// message as ntfy's JSON lines.
pub fn fake_ntfy() -> (String, Topics) {
    fake_ntfy_limited(Arc::default(), Arc::default())
}

/// [`fake_ntfy`] counting every request in `seen`, and answering ntfy.sh's
/// rate-limit 429 (with `Retry-After: 1`) while `refuse` counts down.
pub fn fake_ntfy_limited(refuse: Arc<AtomicUsize>, seen: Arc<AtomicUsize>) -> (String, Topics) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let topics: Topics = Arc::default();
    let store = Arc::clone(&topics);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let store = Arc::clone(&store);
            seen.fetch_add(1, Ordering::SeqCst);
            let refused = refuse
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_owned();
                let target = parts.next().unwrap_or("").to_owned();
                let mut len = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let path = target.split('?').next().unwrap().trim_start_matches('/');
                if refused {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 429 Too Many Requests\r\nretry-after: 1\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    return;
                }
                let reply = if method == "POST" {
                    let mut topics = store.lock().unwrap();
                    topics.entry(path.to_owned()).or_default().push((
                        String::from_utf8(body).unwrap(),
                        rbx_presence::now_ms() / 1000,
                    ));
                    "{}".to_owned()
                } else {
                    let topic = path.trim_end_matches("/json");
                    let topics = store.lock().unwrap();
                    topics
                        .get(topic)
                        .into_iter()
                        .flatten()
                        .map(|(m, time)| {
                            serde_json::json!({ "event": "message", "message": m, "time": time })
                                .to_string()
                                + "\n"
                        })
                        .collect()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            });
        }
    });
    (url, topics)
}

struct Loopback;

impl Tunnel for Loopback {
    fn alive(&mut self) -> bool {
        true
    }
}

pub fn state(user_id: u64, selection: &str) -> UserState {
    UserState {
        v: SCHEMA_VERSION,
        user_id,
        name: format!("User {user_id}"),
        color: rbx_presence::schema::color_for(user_id).to_owned(),
        client: Editor::RbxNative,
        camera: None,
        selection: vec![selection.to_owned()],
        script: None,
        edits: Vec::new(),
        seen: 0,
    }
}

pub fn start(
    ntfy: &str,
    key: &str,
    enc_key: [u8; 32],
    user_id: u64,
    can_host: bool,
) -> (Client, Receiver<Event>) {
    let hosting = can_host.then(|| loopback(Arc::default()));
    start_with(ntfy, key, enc_key, user_id, hosting)
}

/// A host whose tunnel opens count up in `opens`.
pub fn start_counting(
    ntfy: &str,
    key: &str,
    enc_key: [u8; 32],
    user_id: u64,
    opens: Arc<AtomicUsize>,
) -> (Client, Receiver<Event>) {
    start_with(ntfy, key, enc_key, user_id, Some(loopback(opens)))
}

fn loopback(opens: Arc<AtomicUsize>) -> Hosting {
    Hosting(Arc::new(move |port: u16| {
        opens.fetch_add(1, Ordering::SeqCst);
        Ok((
            format!("http://127.0.0.1:{port}"),
            Box::new(Loopback) as Box<dyn Tunnel>,
        ))
    }))
}

pub fn start_with(
    ntfy: &str,
    key: &str,
    enc_key: [u8; 32],
    user_id: u64,
    hosting: Option<Hosting>,
) -> (Client, Receiver<Event>) {
    Client::start(Config {
        kind: RelayKind::Hosted,
        relay_url: ntfy.to_owned(),
        room: auth::room_id(key),
        key: key.to_owned(),
        enc_key,
        user_id,
        place_id: 4567,
        beacon_from: "test".into(),
        hosting,
    })
}
