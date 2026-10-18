//! Cutting a batch into relay-sized pieces and putting it back together.
//! One chunk's plaintext is `{"b": batch id, "i": index, "n": count, "d":
//! base64 of a byte slice of the batch JSON}`; at [`CHUNK_BYTES`] raw bytes a
//! sealed chunk stays well under the relay's 16 KiB body limit.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::crypt::{b64_decode, b64_encode};

/// Raw batch-JSON bytes per chunk.
pub const CHUNK_BYTES: usize = 7000;
/// A sender's unfinished batch is dropped after this many later log entries.
pub const STALE_AFTER: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub b: String,
    pub i: u32,
    pub n: u32,
    pub d: String,
}

/// `batch_json` cut into chunks of at most [`CHUNK_BYTES`] raw bytes, all
/// with batch id `id` (at least one chunk, even for an empty string).
pub fn split(id: &str, batch_json: &str) -> Vec<Chunk> {
    let bytes = batch_json.as_bytes();
    if bytes.is_empty() {
        return vec![Chunk {
            b: id.to_string(),
            i: 0,
            n: 1,
            d: String::new(),
        }];
    }
    let n = bytes.len().div_ceil(CHUNK_BYTES) as u32;
    bytes
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(i, part)| Chunk {
            b: id.to_string(),
            i: i as u32,
            n,
            d: b64_encode(part),
        })
        .collect()
}

/// A batch being collected: the seq of its first chunk, and its chunks so far.
type Pending = (u64, Vec<Option<Vec<u8>>>);

/// Collects chunks per (sender, batch id) in log order.
#[derive(Default)]
pub struct Reassembler {
    pending: HashMap<(String, String), Pending>,
}

impl Reassembler {
    /// Takes the chunk logged at `seq` from `sender`; returns the whole batch
    /// JSON once its last missing chunk arrives. Malformed chunks (bad
    /// base64, i >= n, n disagreeing with earlier chunks, n == 0 or > 4096)
    /// are ignored; batches unfinished for [`STALE_AFTER`] entries are dropped.
    pub fn push(&mut self, seq: u64, sender: &str, chunk: Chunk) -> Option<String> {
        self.pending
            .retain(|_, (first, _)| seq.saturating_sub(*first) < STALE_AFTER);
        if chunk.n == 0 || chunk.n > 4096 || chunk.i >= chunk.n {
            return None;
        }
        let data = b64_decode(&chunk.d)?;
        let key = (sender.to_string(), chunk.b);
        let (_, slots) = self
            .pending
            .entry(key.clone())
            .or_insert_with(|| (seq, vec![None; chunk.n as usize]));
        if slots.len() != chunk.n as usize {
            return None;
        }
        slots[chunk.i as usize] = Some(data);
        if slots.iter().any(Option::is_none) {
            return None;
        }
        let (_, slots) = self.pending.remove(&key)?;
        String::from_utf8(slots.into_iter().flatten().flatten().collect()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(len: usize) -> String {
        (0..len).map(|i| (b'a' + (i % 26) as u8) as char).collect()
    }

    fn feed(r: &mut Reassembler, sender: &str, chunks: Vec<Chunk>) -> Option<String> {
        let mut out = None;
        for (k, c) in chunks.into_iter().enumerate() {
            out = r.push(k as u64 + 1, sender, c).or(out);
        }
        out
    }

    #[test]
    fn split_sizes() {
        for (len, count) in [(0, 1), (1, 1), (7000, 1), (7001, 2), (50000, 8)] {
            let chunks = split("b", &text(len));
            assert_eq!(chunks.len(), count, "{len}");
            assert!(chunks.iter().all(|c| c.n as usize == count && c.b == "b"));
            let mut r = Reassembler::default();
            assert_eq!(feed(&mut r, "1:studio", chunks), Some(text(len)));
        }
    }

    #[test]
    fn out_of_order_reassembles() {
        let mut chunks = split("b", &text(20000));
        chunks.reverse();
        let mut r = Reassembler::default();
        assert_eq!(feed(&mut r, "s", chunks), Some(text(20000)));
    }

    #[test]
    fn interleaved_senders_stay_apart() {
        let a = split("b", &text(15000));
        let b = split("b", &"z".repeat(15000));
        let mut r = Reassembler::default();
        let mut done = Vec::new();
        let mut seq = 0;
        for (x, y) in a.into_iter().zip(b) {
            seq += 1;
            done.extend(r.push(seq, "a", x));
            seq += 1;
            done.extend(r.push(seq, "b", y));
        }
        assert_eq!(done, vec![text(15000), "z".repeat(15000)]);
    }

    #[test]
    fn malformed_chunks_are_ignored() {
        let mut r = Reassembler::default();
        let ok = |i, n| Chunk {
            b: "b".into(),
            i,
            n,
            d: b64_encode(b"x"),
        };
        assert_eq!(
            r.push(
                1,
                "s",
                Chunk {
                    d: "!!".into(),
                    ..ok(0, 1)
                }
            ),
            None
        );
        assert_eq!(r.push(2, "s", ok(1, 1)), None);
        assert_eq!(r.push(3, "s", ok(0, 0)), None);
        assert_eq!(r.push(4, "s", ok(0, 4097)), None);
        assert_eq!(r.push(5, "s", ok(0, 2)), None);
        assert_eq!(r.push(6, "s", ok(0, 3)), None);
        assert_eq!(r.push(7, "s", ok(1, 2)), Some("xx".into()));
    }

    #[test]
    fn stale_batches_are_dropped() {
        let mut r = Reassembler::default();
        let mut chunks = split("b", &text(10000));
        let second = chunks.pop().unwrap();
        assert_eq!(r.push(1, "s", chunks.pop().unwrap()), None);
        assert_eq!(r.push(1 + STALE_AFTER, "s", second), None);
    }
}
