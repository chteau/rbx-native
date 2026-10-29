//! What a peer keeps of the op log it reads, so the log outlives the relay
//! that held it: entries exactly as logged (seq, sender, ciphertext), the
//! log's id, and the half-read batches. When the room moves to a relay that
//! holds none of the log (a new host), or only part of it, the peer hands
//! the rest over (`POST …/ops/fill`) and everyone carries on at the same
//! seqs. The Studio plugin keeps the same copy (`CoeditLog.luau`).

use std::collections::VecDeque;

use super::chunk::Reassembler;
use crate::schema::{LoggedOp, OpsReply};

/// Most ciphertext kept, the relay's own cap; the oldest goes first.
pub const KEPT_BYTES: usize = crate::host::OPS_LOG_BYTES;
/// Most ciphertext per request when posting or filling (the body limit is
/// [`crate::schema::OPS_BODY_BYTES`], with room for the JSON around it).
pub const REQUEST_CT_BYTES: usize = 512 * 1024;

#[derive(Debug, Default)]
pub struct Kept {
    log: String,
    entries: VecDeque<LoggedOp>,
    bytes: usize,
    pub(crate) reassembler: Reassembler,
}

/// What to do with a reply from the relay.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Take its entries and read on.
    Read,
    /// It lacks entries this peer kept: hand them over, then read again.
    Fill(Vec<LoggedOp>),
    /// It runs another log: forget this one and read that from its start.
    Restart,
}

impl Kept {
    pub fn log(&self) -> &str {
        &self.log
    }

    /// The newest seq kept; 0 for none.
    pub fn head(&self) -> u64 {
        self.entries.back().map_or(0, |e| e.seq)
    }

    /// Keeps an entry of log `log` (the reply's), in order.
    pub fn record(&mut self, log: &str, entry: &LoggedOp) {
        if self.log != log {
            self.restart(log);
        }
        self.bytes += entry.ct.len();
        self.entries.push_back(entry.clone());
        while self.bytes > KEPT_BYTES {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.ct.len();
        }
    }

    /// Forgets the kept log for `log`.
    pub fn restart(&mut self, log: &str) {
        *self = Kept {
            log: log.to_owned(),
            ..Kept::default()
        };
    }

    pub fn step(&self, reply: &OpsReply) -> Step {
        if reply.log.is_empty() {
            // A relay that holds nothing yet takes the whole log.
            return if reply.head == 0 && !self.entries.is_empty() {
                Step::Fill(self.entries.iter().cloned().collect())
            } else {
                Step::Read
            };
        }
        if self.log.is_empty() {
            return Step::Read;
        }
        if reply.log != self.log {
            return Step::Restart;
        }
        let tail: Vec<LoggedOp> = self
            .entries
            .iter()
            .filter(|e| e.seq > reply.head)
            .cloned()
            .collect();
        match tail.first() {
            Some(first) if first.seq == reply.head + 1 => Step::Fill(tail),
            _ => Step::Read,
        }
    }
}

/// `items` cut into runs of at most `max` bytes each (by `size`), never
/// empty, so one request each.
pub fn groups<T>(items: Vec<T>, max: usize, size: impl Fn(&T) -> usize) -> Vec<Vec<T>> {
    let mut out: Vec<Vec<T>> = Vec::new();
    let mut used = 0;
    for item in items {
        let n = size(&item);
        match out.last_mut() {
            Some(group) if used + n <= max => group.push(item),
            _ => {
                out.push(vec![item]);
                used = 0;
            }
        }
        used += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Client;

    fn entry(seq: u64) -> LoggedOp {
        LoggedOp {
            seq,
            user_id: 1,
            client: Client::Studio,
            ct: format!("E{seq}"),
        }
    }

    fn kept(log: &str, seqs: std::ops::RangeInclusive<u64>) -> Kept {
        let mut kept = Kept::default();
        for seq in seqs {
            kept.record(log, &entry(seq));
        }
        kept
    }

    fn reply(log: &str, head: u64) -> OpsReply {
        OpsReply {
            head,
            floor: 1,
            log: log.into(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn an_empty_relay_is_handed_the_whole_log() {
        let kept = kept("abc", 1..=3);
        assert_eq!(kept.head(), 3);
        assert_eq!(
            kept.step(&reply("", 0)),
            Step::Fill(vec![entry(1), entry(2), entry(3)])
        );
        assert_eq!(Kept::default().step(&reply("", 0)), Step::Read);
    }

    #[test]
    fn a_relay_behind_on_the_same_log_gets_the_tail() {
        let kept = kept("abc", 1..=5);
        assert_eq!(
            kept.step(&reply("abc", 3)),
            Step::Fill(vec![entry(4), entry(5)])
        );
        assert_eq!(kept.step(&reply("abc", 5)), Step::Read);
        assert_eq!(kept.step(&reply("abc", 9)), Step::Read);
    }

    #[test]
    fn a_tail_the_trim_cut_off_is_not_offered() {
        let kept = kept("abc", 6..=8);
        assert_eq!(kept.step(&reply("abc", 3)), Step::Read);
    }

    #[test]
    fn another_log_restarts_and_the_first_log_is_adopted() {
        let mut kept = kept("abc", 1..=2);
        assert_eq!(kept.step(&reply("xyz", 1)), Step::Restart);
        kept.restart("xyz");
        assert_eq!((kept.head(), kept.log()), (0, "xyz"));
        assert_eq!(Kept::default().step(&reply("xyz", 4)), Step::Read);
        let mut fresh = Kept::default();
        fresh.record("xyz", &entry(1));
        assert_eq!(fresh.log(), "xyz");
    }

    #[test]
    fn groups_stay_under_the_cap_and_keep_order() {
        let sizes = vec![4, 4, 4, 9, 1];
        let got = groups(sizes, 8, |n| *n);
        assert_eq!(got, vec![vec![4, 4], vec![4], vec![9], vec![1]]);
        assert!(groups(Vec::<usize>::new(), 8, |n| *n).is_empty());
    }
}
