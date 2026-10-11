//! PCG32 (O'Neill's `pcg32_random_r`), the generator behind Luau's `Random`.
//! Documented (creator-docs `AnimationNodeType.yaml`, RandomSequenceNode):
//! Seed "ensur[es] the sequence remains consistent across clients" and
//! "defaults to `-1`, which uses a random seed"; `graph-editor.md` lists RNG
//! seeds among the replicated state. UNVERIFIED: the generator and how a seed
//! starts it, which live in the engine (the Studio editor's code in Client
//! Tracker has no runtime); here a seed simply starts the stream, so equal
//! seeds replay equal picks.

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

    pub fn new(seed: u64) -> Pcg32 {
        let mut rng = Pcg32 {
            state: 0,
            inc: (0xDA3E_39CB_94B9_5BDB << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(Self::MULTIPLIER).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }

    pub fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f32) as usize).min(n.saturating_sub(1))
    }
}
