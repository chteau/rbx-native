//! The simulated network between a Local Run server and each of its
//! clients, so replication behaves as it does across a real connection
//! (creator-docs "Remote events and callbacks", `UnreliableRemoteEvent`):
//!
//! - everything travels in frames: what a peer sends during a frame leaves
//!   at its end, and arrives on the frame nearest the link's delay later
//!   (latency, plus or minus up to the jitter);
//! - the reliable stream (property changes, new and removed instances,
//!   `RemoteEvent` and `RemoteFunction` traffic) is ordered: a packet the
//!   link loses is sent again a round trip later, and everything after it
//!   waits behind it;
//! - an `UnreliableRemoteEvent` packet the link loses is gone, and each one
//!   takes its own delay, so they can overtake each other;
//! - physics is unreliable too, but a lost update's poses ride along with
//!   the next one, and a pose older than one already applied is dropped.
//!
//! The default, no added latency and no loss, delivers everything on the
//! frame it was sent, as Local Run always has.

use std::collections::VecDeque;

use rbx_dom::Variant;

use crate::FRAME_HZ;

/// One direction of a link.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Way {
    /// Seconds a packet takes.
    pub latency: f64,
    /// Seconds a packet's delay may stray from `latency`, either way.
    pub jitter: f64,
    /// The share of packets lost, 0 to 1.
    pub loss: f64,
}

/// The network every client has to the server.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Network {
    /// Server to client.
    pub down: Way,
    /// Client to server.
    pub up: Way,
}

impl Network {
    /// The round trip `Player:GetNetworkPing()` reports, in seconds.
    pub fn ping(&self) -> f64 {
        self.down.latency + self.up.latency
    }
}

/// Retransmissions are capped: a link losing everything still delivers.
const MOST_LOSS: f64 = 0.9;

/// Packets in flight one way, each due on a frame.
pub(crate) struct Pipe<T> {
    flight: Vec<(u64, u64, T)>,
    seq: u64,
    /// The frame the last reliable packet is due: later ones wait for it.
    tail: u64,
    rng: u64,
}

impl<T> Pipe<T> {
    pub(crate) fn new(seed: u64) -> Self {
        Pipe {
            flight: Vec::new(),
            seq: 0,
            tail: 0,
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        }
    }

    /// A uniform number in [0, 1) (xorshift64*).
    fn roll(&mut self) -> f64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn frames(seconds: f64) -> u64 {
        (seconds * FRAME_HZ).round().max(0.0) as u64
    }

    fn delay(&mut self, way: &Way) -> u64 {
        let jitter = if way.jitter > 0.0 {
            (self.roll() * 2.0 - 1.0) * way.jitter
        } else {
            0.0
        };
        Self::frames(way.latency + jitter)
    }

    fn lost(&mut self, way: &Way) -> bool {
        way.loss > 0.0 && self.roll() < way.loss.min(MOST_LOSS)
    }

    fn push(&mut self, due: u64, item: T) {
        self.flight.push((due, self.seq, item));
        self.seq += 1;
    }

    /// Sends `item` reliably on frame `now`; `rtt` is the link's round trip,
    /// how long a lost packet takes to be sent again. Returns how many
    /// times it had to be sent again.
    pub(crate) fn reliable(&mut self, now: u64, way: &Way, rtt: f64, item: T) -> u32 {
        let mut due = now + self.delay(way);
        let mut resent = 0;
        while self.lost(way) {
            due += Self::frames(rtt).max(1);
            resent += 1;
        }
        due = due.max(self.tail);
        self.tail = due;
        self.push(due, item);
        resent
    }

    /// Sends `item` unreliably on frame `now`: back if the link lost it.
    pub(crate) fn unreliable(&mut self, now: u64, way: &Way, item: T) -> Result<(), T> {
        if self.lost(way) {
            return Err(item);
        }
        let due = now + self.delay(way);
        self.push(due, item);
        Ok(())
    }

    /// What has arrived by frame `now`, in arrival order.
    pub(crate) fn due(&mut self, now: u64) -> Vec<T> {
        if self.flight.iter().all(|(due, ..)| *due > now) {
            return Vec::new();
        }
        let (mut ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.flight)
            .into_iter()
            .partition(|(due, ..)| *due <= now);
        self.flight = waiting;
        ready.sort_by_key(|(due, seq, _)| (*due, *seq));
        ready.into_iter().map(|(.., item)| item).collect()
    }
}

/// Bytes sent one way, per frame, for `Stats`' rates.
#[derive(Default)]
pub(crate) struct Meter(VecDeque<(u64, usize)>);

impl Meter {
    pub(crate) fn add(&mut self, frame: u64, bytes: usize) {
        match self.0.back_mut() {
            Some((at, total)) if *at == frame => *total += bytes,
            _ => self.0.push_back((frame, bytes)),
        }
    }

    /// Kilobytes a second over the second up to frame `now`.
    pub(crate) fn kbps(&mut self, now: u64) -> f64 {
        let window = FRAME_HZ as u64;
        while self.0.front().is_some_and(|(at, _)| at + window <= now) {
            self.0.pop_front();
        }
        self.0.iter().map(|(_, bytes)| *bytes).sum::<usize>() as f64 / 1000.0
    }
}

/// About what a property value takes on the wire. Roblox's encoding isn't
/// documented; these are the raw sizes of each type's fields.
pub(crate) fn variant_size(value: &Variant) -> usize {
    match value {
        Variant::String(s) => 2 + s.len(),
        Variant::Bool(_) => 1,
        Variant::Float32(_) | Variant::Int32(_) | Variant::Enum(_) | Variant::BrickColor(_) => 4,
        Variant::Float64(_) | Variant::Int64(_) | Variant::Vector2(_) | Variant::Ref(_) => 8,
        Variant::Vector3(_) | Variant::Color3(_) => 12,
        Variant::CFrame(_) => 48,
        _ => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(latency: f64, jitter: f64, loss: f64) -> Way {
        Way {
            latency,
            jitter,
            loss,
        }
    }

    #[test]
    fn nothing_added_arrives_on_the_frame_it_left() {
        let mut pipe = Pipe::new(1);
        pipe.reliable(5, &Way::default(), 0.0, 'a');
        assert!(pipe.unreliable(5, &Way::default(), 'b').is_ok());
        pipe.reliable(5, &Way::default(), 0.0, 'c');
        assert_eq!(pipe.due(5), vec!['a', 'b', 'c']);
    }

    #[test]
    fn latency_counts_in_whole_frames() {
        let mut pipe = Pipe::new(1);
        pipe.reliable(0, &way(0.1, 0.0, 0.0), 0.2, 1);
        assert!(pipe.due(5).is_empty());
        assert_eq!(pipe.due(6), vec![1]);
    }

    #[test]
    fn reliable_packets_stay_in_order_through_jitter_and_loss() {
        let mut pipe = Pipe::new(7);
        let link = way(0.05, 0.04, 0.3);
        for n in 0..500 {
            pipe.reliable(n / 4, &link, 0.1, n);
        }
        let got: Vec<u64> = (0..400).flat_map(|f| pipe.due(f)).collect();
        assert_eq!(got, (0..500).collect::<Vec<_>>());
    }

    #[test]
    fn unreliable_packets_get_lost_and_overtake() {
        let mut pipe = Pipe::new(3);
        let link = way(0.05, 0.04, 0.2);
        let sent = (0..1000)
            .filter(|n| pipe.unreliable(n / 4, &link, *n).is_ok())
            .count();
        let got: Vec<u64> = (0..400).flat_map(|f| pipe.due(f)).collect();
        assert_eq!(got.len(), sent);
        assert!((700..900).contains(&sent), "{sent}");
        assert!(got.windows(2).any(|w| w[1] < w[0]));
    }
}
