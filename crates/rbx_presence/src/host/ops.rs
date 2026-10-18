//! The hosted relay's co-editing op log: an append-only, numbered list of
//! sealed entries that readers fetch by `since`. The relay never opens `ct`.

use std::collections::{HashMap, VecDeque};

use super::room::Refusal;
use crate::schema::{LoggedOp, OpsReply, SealedOp};

/// The log keeps at most this many bytes of `ct`, trimming the oldest.
pub const OPS_LOG_BYTES: usize = 64 * 1024 * 1024;
/// One reply carries at most this many bytes of `ct` (the reader loops).
pub const OPS_REPLY_BYTES: usize = 512 * 1024;
/// Token bucket per user: burst size and refill per second.
pub const OPS_RATE_BURST: f64 = 64.0;
pub const OPS_RATE_PER_SECOND: f64 = 32.0;

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
    buckets: HashMap<u64, Bucket>,
}

impl Default for OpLog {
    fn default() -> Self {
        OpLog::with_cap(OPS_LOG_BYTES)
    }
}

impl OpLog {
    pub fn with_cap(cap: usize) -> Self {
        OpLog {
            entries: VecDeque::new(),
            bytes: 0,
            cap,
            floor: 1,
            head: 0,
            buckets: HashMap::new(),
        }
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    fn take(&mut self, user_id: u64, now: u64) -> bool {
        let bucket = self.buckets.entry(user_id).or_insert(Bucket {
            tokens: OPS_RATE_BURST,
            at: now,
        });
        let elapsed = now.saturating_sub(bucket.at) as f64 / 1000.0;
        bucket.tokens = (bucket.tokens + elapsed * OPS_RATE_PER_SECOND).min(OPS_RATE_BURST);
        bucket.at = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }

    /// Logs `op` and returns its seq; refused when the user is over their rate.
    pub fn append(&mut self, op: SealedOp, now: u64) -> Result<u64, Refusal> {
        if !self.take(op.user_id, now) {
            return Err(Refusal::new(429, "rate"));
        }
        self.head += 1;
        self.bytes += op.ct.len();
        self.entries.push_back(LoggedOp {
            seq: self.head,
            user_id: op.user_id,
            client: op.client,
            ct: op.ct,
        });
        while self.bytes > self.cap {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.ct.len();
        }
        self.floor = self.entries.front().map_or(self.head + 1, |e| e.seq);
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
            entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Client, SCHEMA_VERSION};

    fn op(user_id: u64, ct: &str) -> SealedOp {
        SealedOp {
            v: SCHEMA_VERSION,
            user_id,
            client: Client::Studio,
            ct: ct.to_owned(),
        }
    }

    #[test]
    fn seqs_start_at_one_and_follow_append_order() {
        let mut log = OpLog::default();
        assert_eq!(log.append(op(1, "AA"), 0), Ok(1));
        assert_eq!(log.append(op(2, "BB"), 0), Ok(2));
        let reply = log.since(0, OPS_REPLY_BYTES);
        assert_eq!((reply.head, reply.floor), (2, 1));
        let seqs: Vec<u64> = reply.entries.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, [1, 2]);
        assert_eq!(log.since(1, OPS_REPLY_BYTES).entries[0].ct, "BB");
    }

    #[test]
    fn trimming_at_the_cap_moves_the_floor() {
        let mut log = OpLog::with_cap(8);
        for _ in 0..4 {
            log.append(op(1, "AAAA"), 0).unwrap();
        }
        let reply = log.since(0, OPS_REPLY_BYTES);
        assert_eq!((reply.head, reply.floor), (4, 3));
        assert_eq!(reply.entries.len(), 2);
        assert_eq!(reply.entries[0].seq, 3);
        assert_eq!(log.since(2, OPS_REPLY_BYTES).entries.len(), 2);
    }

    #[test]
    fn an_entry_over_the_cap_empties_the_log() {
        let mut log = OpLog::with_cap(2);
        log.append(op(1, "AAAA"), 0).unwrap();
        let reply = log.since(0, OPS_REPLY_BYTES);
        assert_eq!((reply.head, reply.floor), (1, 2));
        assert!(reply.entries.is_empty());
    }

    #[test]
    fn since_past_head_is_empty() {
        let mut log = OpLog::default();
        log.append(op(1, "AA"), 0).unwrap();
        let reply = log.since(5, OPS_REPLY_BYTES);
        assert!(reply.entries.is_empty());
        assert_eq!(reply.head, 1);
        assert!(log.since(1, OPS_REPLY_BYTES).entries.is_empty());
    }

    #[test]
    fn a_reply_stops_at_its_byte_cap_but_never_empty() {
        let mut log = OpLog::default();
        for _ in 0..3 {
            log.append(op(1, "AAAA"), 0).unwrap();
        }
        assert_eq!(log.since(0, 8).entries.len(), 2);
        assert_eq!(log.since(0, 1).entries.len(), 1);
    }

    #[test]
    fn the_rate_limit_refuses_past_the_burst_and_refills() {
        let mut log = OpLog::default();
        for _ in 0..64 {
            log.append(op(1, "AA"), 1000).unwrap();
        }
        assert_eq!(
            log.append(op(1, "AA"), 1000),
            Err(Refusal::new(429, "rate"))
        );
        assert!(log.append(op(2, "AA"), 1000).is_ok());
        assert!(log.append(op(1, "AA"), 1100).is_ok());
    }
}
