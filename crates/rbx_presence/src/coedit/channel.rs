//! The op log over HTTP, on worker threads: sends batches (chunked, sealed,
//! one signed `POST /v1/rooms/<room>/ops` per chunk, in order, retrying with
//! backoff) and long-polls `GET /v1/rooms/<room>/ops?since=N` from 0, so a
//! late joiner replays the whole log.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::chunk::{self, Chunk, Reassembler};
use super::Batch;
use crate::crypt;
use crate::schema::{Client as Editor, LoggedOp, OpsReply, SealedOp, SCHEMA_VERSION};
use crate::{auth, now_ms};

const BACKOFF_START: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
const STOP_TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct Config {
    /// The relay's base URL (`https://…`, or `http://127.0.0.1:…` in tests).
    pub relay_url: String,
    pub room: String,
    pub key: String,
    pub enc_key: [u8; 32],
    pub user_id: u64,
    pub client: Editor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A whole batch, at the log position of its last chunk, from
    /// `user_id`/`client` (this editor's own included).
    Batch {
        seq: u64,
        user_id: u64,
        client: Editor,
        batch: Batch,
    },
    /// The relay no longer holds entries this editor has not seen (its log
    /// was trimmed); edits were missed and the place should be reopened.
    Gap,
    /// A request failed; the threads keep retrying.
    Error(String),
}

/// The handle; dropping it stops both threads.
pub struct Channel {
    batches: std::sync::mpsc::Sender<Batch>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// Sleeps `total`, waking early when `stop` is set.
fn nap(stop: &AtomicBool, total: Duration) {
    let end = Instant::now() + total;
    while !stop.load(Ordering::SeqCst) {
        let Some(left) = end.checked_duration_since(Instant::now()) else {
            return;
        };
        thread::sleep(left.min(STOP_TICK));
    }
}

fn base(config: &Config) -> &str {
    config.relay_url.trim_end_matches('/')
}

fn ops_path(config: &Config) -> String {
    format!("/v1/rooms/{}/ops", config.room)
}

fn post(agent: &ureq::Agent, config: &Config, chunk: &Chunk) -> Result<(), String> {
    let plain = serde_json::to_vec(chunk).map_err(|e| e.to_string())?;
    let aad = crypt::ops_aad(&config.room, config.user_id, config.client);
    let sealed = SealedOp {
        v: SCHEMA_VERSION,
        user_id: config.user_id,
        client: config.client,
        ct: crypt::b64_encode(&crypt::seal(&config.enc_key, &aad, &plain)),
    };
    let body = serde_json::to_string(&sealed).map_err(|e| e.to_string())?;
    let path = ops_path(config);
    let mut req = agent
        .post(format!("{}{path}", base(config)))
        .header("Content-Type", "application/json");
    for (name, value) in auth::headers(
        &config.key,
        "POST",
        &path,
        config.user_id,
        now_ms(),
        body.as_bytes(),
        true,
    ) {
        req = req.header(name, &value);
    }
    let resp = req.send(body.as_str()).map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("ops post: HTTP {status}"))
    }
}

fn fetch(agent: &ureq::Agent, config: &Config, since: u64) -> Result<OpsReply, String> {
    let path = format!("{}?since={since}", ops_path(config));
    let mut req = agent.get(format!("{}{path}", base(config)));
    for (name, value) in auth::headers(
        &config.key,
        "GET",
        &path,
        config.user_id,
        now_ms(),
        b"",
        true,
    ) {
        req = req.header(name, &value);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("ops poll: HTTP {status}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("ops reply: {e}"))
}

fn batch_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    hex::encode(bytes)
}

fn send_loop(
    config: Config,
    batches: Receiver<Batch>,
    events: Sender<Event>,
    stop: Arc<AtomicBool>,
) {
    let agent = agent();
    let mut failing = false;
    let mut backoff = BACKOFF_START;
    while !stop.load(Ordering::SeqCst) {
        let batch = match batches.recv_timeout(Duration::from_millis(250)) {
            Ok(batch) => batch,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Ok(json) = serde_json::to_string(&batch) else {
            continue;
        };
        for piece in chunk::split(&batch_id(), &json) {
            loop {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                match post(&agent, &config, &piece) {
                    Ok(()) => {
                        failing = false;
                        backoff = BACKOFF_START;
                        break;
                    }
                    Err(why) => {
                        if !failing {
                            failing = true;
                            let _ = events.send(Event::Error(why));
                        }
                        nap(&stop, backoff);
                        backoff = (backoff * 2).min(BACKOFF_MAX);
                    }
                }
            }
        }
    }
}

fn deliver(
    entry: &LoggedOp,
    config: &Config,
    reassembler: &mut Reassembler,
    events: &Sender<Event>,
) {
    let aad = crypt::ops_aad(&config.room, entry.user_id, entry.client);
    let Some(plain) =
        crypt::b64_decode(&entry.ct).and_then(|ct| crypt::open(&config.enc_key, &aad, &ct))
    else {
        return;
    };
    let Ok(piece) = serde_json::from_slice::<Chunk>(&plain) else {
        return;
    };
    let sender = format!("{}:{}", entry.user_id, crypt::client_word(entry.client));
    let Some(json) = reassembler.push(entry.seq, &sender, piece) else {
        return;
    };
    if let Ok(batch) = serde_json::from_str::<Batch>(&json) {
        let _ = events.send(Event::Batch {
            seq: entry.seq,
            user_id: entry.user_id,
            client: entry.client,
            batch,
        });
    }
}

fn poll_loop(config: Config, events: Sender<Event>, stop: Arc<AtomicBool>) {
    let agent = agent();
    let mut reassembler = Reassembler::default();
    let mut since = 0u64;
    let mut failing = false;
    let mut backoff = BACKOFF_START;
    while !stop.load(Ordering::SeqCst) {
        let reply = match fetch(&agent, &config, since) {
            Ok(reply) => reply,
            Err(why) => {
                if !failing {
                    failing = true;
                    let _ = events.send(Event::Error(why));
                }
                nap(&stop, backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        failing = false;
        backoff = BACKOFF_START;
        if since + 1 < reply.floor {
            let _ = events.send(Event::Gap);
            since = reply.floor - 1;
        }
        for entry in &reply.entries {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if entry.seq <= since {
                continue;
            }
            since = entry.seq;
            deliver(entry, &config, &mut reassembler, &events);
        }
        if reply.entries.is_empty() {
            nap(&stop, Duration::from_millis(50));
        }
    }
}

impl Channel {
    pub fn start(config: Config) -> (Channel, Receiver<Event>) {
        let (batch_tx, batch_rx) = channel();
        let (event_tx, event_rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));

        let sender_config = config.clone();
        let sender_events = event_tx.clone();
        let sender_stop = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("coedit-send".into())
            .spawn(move || send_loop(sender_config, batch_rx, sender_events, sender_stop));

        let poll_stop = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("coedit-poll".into())
            .spawn(move || poll_loop(config, event_tx, poll_stop));

        (
            Channel {
                batches: batch_tx,
                stop,
            },
            event_rx,
        )
    }

    pub fn send(&self, batch: Batch) {
        let _ = self.batches.send(batch);
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
