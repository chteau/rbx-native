//! Particle support: reading emitters out of a DOM ([`emitter`], plus
//! [`legacy`] for the preconfigured `Fire`/`Smoke`/`Sparkles` classes) and
//! simulating them on the CPU ([`sim`]), independently of any GPU state.
//!
//! [`Scene::particle_emitters`](super::Scene::particle_emitters) is the only
//! door in; the renderer owns one [`Simulation`] per [`Emitter`] it gets back
//! and steps it every frame with its own `dt` — see `renderer::particles`.

mod emitter;
mod legacy;
mod rng;
// `pub(crate)`: `scene::beam` samples the same ColorSequence/NumberSequence
// shape along a beam's length, so its evaluator is shared rather than copied.
pub(crate) mod sequence;
mod sim;

pub(crate) use emitter::{plan, Emitter};
pub(crate) use sim::Simulation;
