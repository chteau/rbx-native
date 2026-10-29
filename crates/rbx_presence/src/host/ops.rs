//! The co-editing op log: an append-only, numbered list of sealed entries
//! that readers fetch by `since`. The relay never opens `ct`.
//!
//! A log is named by a random id minted when its first entry lands. A
//! relay that starts with no log, a new host after the old one left, can
//! instead be handed one by a peer that read it (`fill`): the entries keep
//! their seqs, senders and ciphertext, so every peer carries on from where
//! it was. To give such a peer the chance, a host's empty log refuses new
//! posts for [`CLAIM_GRACE_MS`] after its first request (`409 log`, which
//! clients retry); the first fill or, after that, the first post claims it.

use std::collections::{HashMap, VecDeque};

use super::room::Refusal;
use crate::schema::{Client, LoggedOp, OpsReply};

/// The log keeps at most this many bytes of `ct`, trimming the oldest.
pub const OPS_LOG_BYTES: usize = 64 * 1024 * 1024;
/// One reply carries at most this many bytes of `ct` (the reader loops).
pub const OPS_REPLY_BYTES: usize = 512 * 1024;
/// Token bucket per user, one token per request: burst size and refill per
/// second.
pub const OPS_RATE_BURST: f64 = 64.0;
pub const OPS_RATE_PER_SECOND: f64 = 32.0;
/// How long a host's empty log waits, from its first request, for a peer to
/// hand it the log the room was using. Peers find a new host within seconds
/// of its announcement, and this only delays the first edit of a room
/// nobody has a log for.
pub const CLAIM_GRACE_MS: u64 = 30_000;

const REFUSED_LOG: Refusal = Refusal::new(409, "log");

struct Bucket {
    tokens: f64,
    at: u64,
}

pub struct OpLog {
    entries: VecDeque<LoggedOp>,
    bytes: usize,
    cap: usize,
    floor: u64,
    head: u64,
    /// Empty until the first entry lands.
    log: String,
    grace: u64,
    opened: Option<u64>,
    buckets: HashMap<u64, Bucket>,
    /// Signatures of posts taken within the clock-skew window, and when.
    seen: HashMap<String, u64>,
}

impl Default for OpLog {
    fn default() -> Self {
        OpLog::new(OPS_LOG_BYTES, 0)
    }
}

fn new_log_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    hex::encode(bytes)
}

impl OpLog {
    /// A log of at most `cap` bytes whose first post waits `grace` ms.
    pub fn new(cap: usize, grace: u64) -> Self {
        OpLog {
            entries: VecDeque::new(),
            bytes: 0,
            cap,
            floor: 1,
            head: 0,
            log: String::new(),
            grace,
            opened: None,
            buckets: HashMap::new(),
            seen: HashMap::new(),
        }
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    /// The log's id; empty while it holds nothing.
    pub fn log(&self) -> &str {
        &self.log
    }

    /// Notes a request at `now`; the claim grace runs from the first.
    pub fn opened(&mut self, now: u64) {
        self.opened.get_or_insert(now);
    }

    fn take(&mut self, user_id: u64, now: u64) -> Result<(), Refusal> {
        let bucket = self.buckets.entry(user_id).or_insert(Bucket {
            tokens: OPS_RATE_BURST,
            at: now,
        });
        let elapsed = now.saturating_sub(bucket.at) as f64 / 1000.0;
        bucket.tokens = (bucket.tokens + elapsed * OPS_RATE_PER_SECOND).min(OPS_RATE_BURST);
        bucket.at = now;
        if bucket.tokens < 1.0 {
            return Err(Refusal::new(429, "rate"));
        }
        bucket.tokens -= 1.0;
        Ok(())
    }

    /// Refuses a post whose signature was already taken: a captured request
    /// sent again. Anything older than the clock-skew window is refused on
    /// its timestamp already, so only that window is remembered.
    pub fn fresh(&mut self, sig: &str, now: u64) -> Result<(), Refusal> {
        let window = 2 * super::room::CLOCK_SKEW_MS;
        self.seen.retain(|_, at| now.saturating_sub(*at) <= window);
        if self.seen.insert(sig.to_owned(), now).is_some() {
            return Err(Refusal::new(401, "replay"));
        }
        Ok(())
    }

    fn push(&mut self, entry: LoggedOp) {
        self.head = entry.seq;
        self.bytes += entry.ct.len();
        self.entries.push_back(entry);
        while self.bytes > self.cap {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.ct.len();
        }
        self.floor = self.entries.front().map_or(self.head + 1, |e| e.seq);
    }

    /// Logs `cts` from one sender under consecutive seqs and returns the
    /// last; refused over the sender's rate, or while an empty log is still
    /// waiting to be filled.
    pub fn append(
        &mut self,
        user_id: u64,
        client: Client,
        cts: Vec<String>,
        now: u64,
    ) -> Result<u64, Refusal> {
        self.opened(now);
        if self.log.is_empty() {
            if self.opened.is_some_and(|at| now < at + self.grace) {
                return Err(REFUSED_LOG);
            }
            self.log = new_log_id();
        }
        self.take(user_id, now)?;
        for ct in cts {
            self.push(LoggedOp {
                seq: self.head + 1,
                user_id,
                client,
                ct,
            });
        }
        Ok(self.head)
    }

    /// Takes entries of log `log` a peer read earlier (consecutive seqs,
    /// checked by the caller): the whole log when this one holds nothing
    /// yet, otherwise those past the head of the same log. Entries already
    /// held are skipped, so two peers filling the same tail is harmless.
    /// Answers the new head; `409 log` for another log or a gap.
    pub fn fill(
        &mut self,
        user_id: u64,
        log: &str,
        entries: Vec<LoggedOp>,
        now: u64,
    ) -> Result<u64, Refusal> {
        let Some(first) = entries.first().map(|e| e.seq) else {
            return Ok(self.head);
        };
        if self.log.is_empty() && self.head == 0 {
            self.take(user_id, now)?;
            self.log = log.to_owned();
            self.head = first - 1;
            self.floor = first;
        } else if self.log != log || first > self.head + 1 {
            return Err(REFUSED_LOG);
        } else {
            self.take(user_id, now)?;
        }
        for entry in entries {
            if entry.seq > self.head {
                self.push(entry);
            }
        }
        Ok(self.head)
    }

    /// Entries after `n`, oldest first, holding at most `max_bytes` of `ct`
    /// (always at least one entry when any is due, so a reader cannot stall).
    pub fn since(&self, n: u64, max_bytes: usize) -> OpsReply {
        let skip = n
            .saturating_add(1)
            .saturating_sub(self.floor)
            .min(self.entries.len() as u64) as usize;
        let mut entries = Vec::new();
        let mut used = 0usize;
        for entry in self.entries.iter().skip(skip) {
            if !entries.is_empty() && used + entry.ct.len() > max_bytes {
                break;
            }
            used += entry.ct.len();
            entries.push(entry.clone());
        }
        OpsReply {
            head: self.head,
            floor: self.floor,
            log: self.log.clone(),
            entries,
        }
    }
}

#[cfg(test)]
mod tests;
