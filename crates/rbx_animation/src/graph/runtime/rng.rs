//! The random number generator RandomSequence draws from: the standard
//! 32-bit Mersenne Twister (C++'s `std::mt19937`) seeded with the node's
//! Seed, one draw `u = x / 2³²` per pick. Measured in real Studio
//! (`tests/studio_graph_probe.rs`, `docs/research/animation-graph-measured.md`):
//! for Seeds 0, 1, 2, 3, 7, 42, 1000 and 123456, over 2, 3 and 5 inputs and
//! weights 1:2:3, all 320 picks are the input whose share of the summed
//! weights holds `u`. The Seed is rounded to an integer and taken modulo
//! 2³² (7.5 picks as 8, −2 as 4294967294); −1 is a fresh seed each time
//! (creator-docs `AnimationNodeType.yaml`), and the node's name plays no
//! part. Luau's `Random` (PCG32) is not what the engine uses here.

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mt19937 {
    state: Box<[u32; Mt19937::N]>,
    at: usize,
}

impl Mt19937 {
    const N: usize = 624;
    const M: usize = 397;

    pub fn new(seed: u32) -> Mt19937 {
        let mut state = Box::new([0u32; Self::N]);
        state[0] = seed;
        for i in 1..Self::N {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Mt19937 { state, at: Self::N }
    }

    /// The generator a node's Seed (anything but −1) starts.
    pub fn from_seed(seed: f64) -> Mt19937 {
        Mt19937::new(seed.round() as i64 as u32)
    }

    fn twist(&mut self) {
        for i in 0..Self::N {
            let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % Self::N] & 0x7fff_ffff);
            let mag = if y & 1 == 1 { 0x9908_b0df } else { 0 };
            self.state[i] = self.state[(i + Self::M) % Self::N] ^ (y >> 1) ^ mag;
        }
        self.at = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.at >= Self::N {
            self.twist();
        }
        let mut y = self.state[self.at];
        self.at += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.
    }
}

#[cfg(test)]
mod tests {
    use super::Mt19937;

    #[test]
    fn matches_the_reference_mt19937() {
        // The C++ standard's check: the 10000th draw of a default-seeded
        // (5489) std::mt19937 is 4123659995.
        let mut rng = Mt19937::new(5489);
        let last = (0..10_000).map(|_| rng.next_u32()).last();
        assert_eq!(last, Some(4_123_659_995));
    }

    #[test]
    fn seeds_round_and_wrap_as_studio_s() {
        let draws = |seed: f64| {
            let mut rng = Mt19937::from_seed(seed);
            (0..8).map(|_| rng.next_u32()).collect::<Vec<_>>()
        };
        assert_eq!(draws(7.5), draws(8.));
        assert_eq!(draws(-2.), {
            let mut rng = Mt19937::new(4_294_967_294);
            (0..8).map(|_| rng.next_u32()).collect::<Vec<_>>()
        });
    }
}
