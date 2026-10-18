//! User-space delay line for the Windows shaper.
//!
//! Each packet is stamped with a release time of `now + latency + uniform
//! [0, jitter]` and held until then. The release time is clamped to be no
//! earlier than the previous packet's (`max(computed, previous)`), so the
//! line is FIFO and jitter never reorders packets. Studio's simulator is not
//! documented to reorder, and a reordered RakNet stream would add
//! head-of-line stalls that are not part of the requested conditions. The
//! cost is that bursts are skewed toward the later end of the jitter range:
//! a packet that rolled a small delay behind one that rolled a large delay
//! inherits the large one.
//!
//! The module is pure (the caller supplies the clock), so it is testable
//! without a driver.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::OneWay;

/// xorshift64*: tiny, fast and plenty for loss/jitter rolls.
pub(crate) struct Rng(u64);

/// Any non-zero constant: xorshift is stuck forever at state 0.
const ZERO_SEED_REPLACEMENT: u64 = 0x9E37_79B9_7F4A_7C15;

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(if seed == 0 {
            ZERO_SEED_REPLACEMENT
        } else {
            seed
        })
    }

    pub(crate) fn from_entropy() -> Rng {
        let mut bytes = [0u8; 8];
        if getrandom::fill(&mut bytes).is_ok() {
            return Rng::new(u64::from_le_bytes(bytes));
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Rng::new(nanos)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)` from the top 53 bits (an f64's mantissa width).
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Beyond this `admit` drops, which bounds memory if the sender stalls.
pub(crate) const MAX_QUEUED: usize = 16_384;

pub(crate) struct DelayLine<T> {
    queue: VecDeque<(Instant, T)>,
    last: Option<Instant>,
}

impl<T> DelayLine<T> {
    pub(crate) fn new() -> Self {
        DelayLine {
            queue: VecDeque::new(),
            last: None,
        }
    }

    /// `None` means dropped (loss roll or full queue); otherwise the release
    /// time. `way` is used as given; callers pass clamped values.
    pub(crate) fn admit(
        &mut self,
        now: Instant,
        way: OneWay,
        rng: &mut Rng,
        item: T,
    ) -> Option<Instant> {
        if self.queue.len() >= MAX_QUEUED || rng.unit() * 100.0 < way.loss_percent {
            return None;
        }
        let extra_ms = way.latency_ms + rng.unit() * way.jitter_ms;
        let mut release = now + Duration::from_secs_f64(extra_ms.max(0.0) / 1000.0);
        if let Some(last) = self.last {
            release = release.max(last);
        }
        self.last = Some(release);
        self.queue.push_back((release, item));
        Some(release)
    }

    pub(crate) fn pop_due(&mut self, now: Instant) -> Option<T> {
        match self.queue.front() {
            Some((release, _)) if *release <= now => self.queue.pop_front().map(|(_, t)| t),
            _ => None,
        }
    }

    pub(crate) fn next_release(&self) -> Option<Instant> {
        self.queue.front().map(|(release, _)| *release)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: f64) -> Duration {
        Duration::from_secs_f64(v / 1000.0)
    }

    #[test]
    fn zero_conditions_release_at_now() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(1);
        let now = Instant::now();
        assert_eq!(line.admit(now, OneWay::default(), &mut rng, 1), Some(now));
        assert_eq!(line.pop_due(now), Some(1));
    }

    #[test]
    fn latency_only() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(2);
        let now = Instant::now();
        let way = OneWay::new(50.0, 0.0, 0.0);
        let release = line.admit(now, way, &mut rng, 1).unwrap();
        assert_eq!(release, now + ms(50.0));
        assert_eq!(line.pop_due(now + ms(49.0)), None);
        assert_eq!(line.pop_due(now + ms(50.0)), Some(1));
    }

    #[test]
    fn jitter_stays_in_range_and_never_decreases() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(3);
        let start = Instant::now();
        let way = OneWay::new(40.0, 30.0, 0.0);
        let mut prev = start;
        for i in 0..1000u32 {
            let now = start + Duration::from_secs(u64::from(i));
            let release = line.admit(now, way, &mut rng, i).unwrap();
            assert!(release >= now + ms(40.0) - Duration::from_micros(1));
            assert!(release <= now + ms(70.0) + Duration::from_micros(1));
            assert!(release >= prev);
            prev = release;
            line.pop_due(release);
        }
    }

    #[test]
    fn burst_never_reorders() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(4);
        let now = Instant::now();
        let way = OneWay::new(0.0, 100.0, 0.0);
        let mut prev = now;
        for i in 0..500u32 {
            let release = line.admit(now, way, &mut rng, i).unwrap();
            assert!(release >= prev);
            prev = release;
        }
        let mut expect = 0;
        while let Some(v) = line.pop_due(now + Duration::from_secs(1)) {
            assert_eq!(v, expect);
            expect += 1;
        }
        assert_eq!(expect, 500);
    }

    #[test]
    fn loss_rate_matches_request() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(0xDEAD_BEEF);
        let now = Instant::now();
        let way = OneWay::new(0.0, 0.0, 0.5);
        let total = 200_000u32;
        let mut dropped = 0u32;
        for i in 0..total {
            if line.admit(now, way, &mut rng, i).is_none() {
                dropped += 1;
            }
            line.pop_due(now);
        }
        let pct = f64::from(dropped) / f64::from(total) * 100.0;
        assert!((0.4..0.6).contains(&pct), "loss was {pct}%");
    }

    #[test]
    fn zero_loss_drops_nothing() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(5);
        let now = Instant::now();
        for i in 0..10_000u32 {
            assert!(line.admit(now, OneWay::default(), &mut rng, i).is_some());
            line.pop_due(now);
        }
    }

    #[test]
    fn pop_due_order_and_next_release() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(6);
        let now = Instant::now();
        assert!(line.is_empty());
        assert_eq!(line.next_release(), None);
        let way = OneWay::new(10.0, 0.0, 0.0);
        line.admit(now, way, &mut rng, 'a');
        line.admit(now + ms(5.0), way, &mut rng, 'b');
        assert_eq!(line.len(), 2);
        assert_eq!(line.next_release(), Some(now + ms(10.0)));
        assert_eq!(line.pop_due(now + ms(9.0)), None);
        assert_eq!(line.pop_due(now + ms(20.0)), Some('a'));
        assert_eq!(line.next_release(), Some(now + ms(15.0)));
        assert_eq!(line.pop_due(now + ms(20.0)), Some('b'));
        assert_eq!(line.pop_due(now + ms(20.0)), None);
        assert!(line.is_empty());
    }

    #[test]
    fn queue_is_capped() {
        let mut line = DelayLine::new();
        let mut rng = Rng::new(7);
        let now = Instant::now();
        let way = OneWay::new(1000.0, 0.0, 0.0);
        for i in 0..MAX_QUEUED {
            assert!(line.admit(now, way, &mut rng, i).is_some());
        }
        assert!(line.admit(now, way, &mut rng, 0).is_none());
        assert_eq!(line.len(), MAX_QUEUED);
        line.pop_due(now + Duration::from_secs(2));
        assert!(line.admit(now, way, &mut rng, 0).is_some());
    }

    #[test]
    fn rng_unit_in_range_and_zero_seed_works() {
        let mut rng = Rng::new(0);
        let mut seen_distinct = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let u = rng.unit();
            assert!((0.0..1.0).contains(&u));
            seen_distinct.insert(u.to_bits());
        }
        assert!(seen_distinct.len() > 9_000);
        assert_ne!(Rng::from_entropy().next_u64(), 0);
    }
}
