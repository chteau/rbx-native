//! The op log over HTTP, on worker threads: sends batches (chunked, sealed,
//! as many chunks per signed `POST /v1/rooms/<room>/ops` as fit, in order,
//! retrying with backoff) and long-polls `GET /v1/rooms/<room>/ops?since=N`,
//! from 0 for a late joiner, so it replays the whole log. What it reads is
//! kept ([`Kept`]) across relays: a relay that lacks the log is handed it.

mod http;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::chunk::{self, Chunk};
use super::keep::{groups, Kept, Step, REQUEST_CT_BYTES};
use super::Batch;
use crate::crypt;
use crate::schema::{Client as Editor, LoggedOp};
use http::{agent, fetch, fill, post, seal};

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
    /// The log as read so far; outlives this relay's channel.
    pub kept: Arc<Mutex<Kept>>,
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

/// Batches waiting to be posted, oldest first; one leaves only once all its
/// chunks are in the log.
type Unsent = Arc<Mutex<VecDeque<Batch>>>;

/// The handle; dropping it stops both threads.
pub struct Channel {
    unsent: Unsent,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

fn batch_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    hex::encode(bytes)
}

fn pop(unsent: &Unsent) {
    if let Ok(mut queue) = unsent.lock() {
        queue.pop_front();
    }
}

fn send_loop(config: Config, unsent: Unsent, events: Sender<Event>, stop: Arc<AtomicBool>) {
    let agent = agent();
    let mut failing = false;
    let mut backoff = BACKOFF_START;
    while !stop.load(Ordering::SeqCst) {
        let Some(batch) = unsent.lock().ok().and_then(|q| q.front().cloned()) else {
            nap(&stop, Duration::from_millis(50));
            continue;
        };
        let Ok(json) = serde_json::to_string(&batch) else {
            pop(&unsent);
            continue;
        };
        let sealed: Result<Vec<String>, String> = chunk::split(&batch_id(), &json)
            .iter()
            .map(|piece| seal(&config, piece))
            .collect();
        let Ok(sealed) = sealed else {
            pop(&unsent);
            continue;
        };
        for group in groups(sealed, REQUEST_CT_BYTES, String::len) {
            loop {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                match post(&agent, &config, group.clone()) {
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
        pop(&unsent);
    }
}

fn deliver(entry: &LoggedOp, config: &Config, kept: &mut Kept, events: &Sender<Event>) {
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
    let Some(json) = kept.reassembler.push(entry.seq, &sender, piece) else {
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

/// Hands the relay `entries` of `log`, a request per group.
fn hand_over(
    agent: &ureq::Agent,
    config: &Config,
    log: &str,
    entries: Vec<LoggedOp>,
) -> Result<(), String> {
    for group in groups(entries, REQUEST_CT_BYTES, |e| e.ct.len()) {
        fill(agent, config, log, group)?;
    }
    Ok(())
}

fn poll_loop(config: Config, events: Sender<Event>, stop: Arc<AtomicBool>) {
    let agent = agent();
    // Where this editor was in the log, if the relay carries on with it.
    let mut since = lock(&config.kept).head();
    let mut failing = false;
    let mut backoff = BACKOFF_START;
    while !stop.load(Ordering::SeqCst) {
        let fetched = fetch(&agent, &config, since).and_then(|reply| {
            let step = lock(&config.kept).step(&reply);
            match step {
                Step::Fill(entries) => {
                    let log = lock(&config.kept).log().to_owned();
                    hand_over(&agent, &config, &log, entries).map(|()| None)
                }
                Step::Restart => {
                    lock(&config.kept).restart(&reply.log);
                    since = 0;
                    Ok(None)
                }
                Step::Read => Ok(Some(reply)),
            }
        });
        let reply = match fetched {
            Ok(Some(reply)) => reply,
            Ok(None) => continue,
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
        let mut kept = lock(&config.kept);
        for entry in &reply.entries {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if entry.seq <= since {
                continue;
            }
            since = entry.seq;
            kept.record(&reply.log, entry);
            deliver(entry, &config, &mut kept, &events);
        }
        drop(kept);
        if reply.entries.is_empty() {
            nap(&stop, Duration::from_millis(50));
        }
    }
}

fn lock(kept: &Mutex<Kept>) -> std::sync::MutexGuard<'_, Kept> {
    kept.lock().unwrap_or_else(|e| e.into_inner())
}

impl Channel {
    pub fn start(config: Config) -> (Channel, Receiver<Event>) {
        let unsent: Unsent = Arc::default();
        let (event_tx, event_rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));

        let sender_config = config.clone();
        let sender_events = event_tx.clone();
        let sender_stop = Arc::clone(&stop);
        let sender_unsent = Arc::clone(&unsent);
        let _ = thread::Builder::new()
            .name("coedit-send".into())
            .spawn(move || send_loop(sender_config, sender_unsent, sender_events, sender_stop));

        let poll_stop = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("coedit-poll".into())
            .spawn(move || poll_loop(config, event_tx, poll_stop));

        (Channel { unsent, stop }, event_rx)
    }

    pub fn send(&self, batch: Batch) {
        if let Ok(mut queue) = self.unsent.lock() {
            queue.push_back(batch);
        }
    }

    /// Stops the threads and hands back what never fully reached the log,
    /// for an editor that moves to another relay (its batches would be lost
    /// with this one otherwise).
    pub fn close(self) -> Vec<Batch> {
        self.stop.store(true, Ordering::SeqCst);
        match self.unsent.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A relay that never answers keeps the batch; closing the channel hands
    /// it back for the next relay.
    #[test]
    fn closing_hands_back_what_never_reached_the_log() {
        let (channel, _events) = Channel::start(Config {
            relay_url: "http://127.0.0.1:1".into(),
            room: "room".into(),
            key: "key".into(),
            enc_key: [7; 32],
            user_id: 1,
            client: Editor::RbxNative,
            kept: Arc::default(),
        });
        let batch = Batch {
            a: "1:rbx-native:x".into(),
            ops: Vec::new(),
        };
        channel.send(batch.clone());
        thread::sleep(Duration::from_millis(300));
        assert_eq!(channel.close(), vec![batch]);
    }
}
