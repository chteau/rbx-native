//! Local Run: a place simulated headless, with no Roblox servers. Server
//! `Script`s run on `rbx_lua`'s engine ([`rbx_lua::server::ServerVm`]),
//! parts move under Rapier ([`physics::World`]), and [`Simulation`] steps
//! both through Roblox's frame, 60 frames a second.
//!
//! The plan, the sources and what's left: `docs/research/local-run/`.

pub mod animation;
pub mod audio;
mod audio_api;
pub mod audio_graph;

/// What a script asked of a `ParticleEmitter` beyond its properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Burst {
    /// `Emit(count)`: that many particles at once, enabled or not.
    Emit(u32),
    /// `Clear()`: every particle in the air goes.
    Clear,
}
pub mod character;
mod chat;
pub mod conformance;
mod engine;
mod explosion;
pub mod gui;
pub mod humanoid;
pub mod input;
mod joints;
mod journal;
pub mod link;
pub mod live;
pub mod load;
mod modules;
pub mod offline_pack;
pub mod ownership;
pub mod physics;
pub mod players;
pub mod replay;
mod scripts;
pub mod session;
mod sim;
mod sound;
mod spatial;
mod streaming;
mod teams;
mod terrain;
mod tools;
mod touch;
mod video;

pub use input::{Input, MouseBehavior, Phase, Sensor};
pub use players::Avatars;
pub use rbx_lua::{LineKind, Output};
pub use session::{Peer, Session, Teleport};
pub use sim::{Role, Simulation, FRAME_HZ};
