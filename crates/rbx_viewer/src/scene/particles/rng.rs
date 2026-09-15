//! A tiny deterministic RNG for particle simulation.
//!
//! Not a real dependency: xorshift64* is a handful of lines and gives every
//! emitter a reproducible stream, which is what makes a `--screenshot` after a
//! fixed pre-warm duration reproducible across runs.

/// xorshift64* state, seeded per emitter (see [`super::emitter::seed_of`]).
pub(crate) struct Rng(u64);

impl Rng {
    /// Zero is the one state xorshift can never leave, so it is nudged to 1.
    pub(crate) fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform float in `[0, 1)`, built from the high bits: xorshift's low bits
    /// are the weakest part of its period.
    fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Uniform float in `[min, max]`, or `min` itself when the range is empty or
    /// inverted — a place file can set either.
    pub(crate) fn range(&mut self, min: f32, max: f32) -> f32 {
        if max <= min {
            min
        } else {
            min + self.next_f32() * (max - min)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_always_replays_the_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let sequence_a: Vec<f32> = (0..5).map(|_| a.range(0.0, 1.0)).collect();
        let sequence_b: Vec<f32> = (0..5).map(|_| b.range(0.0, 1.0)).collect();
        assert_eq!(sequence_a, sequence_b);
    }

    #[test]
    fn a_zero_seed_still_produces_a_moving_stream() {
        let mut rng = Rng::new(0);
        let first = rng.range(0.0, 1.0);
        let second = rng.range(0.0, 1.0);
        assert_ne!(first, second);
    }

    #[test]
    fn range_stays_within_its_bounds() {
        let mut rng = Rng::new(7);
        for _ in 0..100 {
            let value = rng.range(-2.0, 5.0);
            assert!((-2.0..=5.0).contains(&value));
        }
    }

    #[test]
    fn an_inverted_or_empty_range_collapses_to_its_minimum() {
        let mut rng = Rng::new(9);
        assert_eq!(rng.range(3.0, 3.0), 3.0);
        assert_eq!(rng.range(5.0, 1.0), 5.0);
    }
}
