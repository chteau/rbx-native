//! Local Run: a place simulated headless, with no Roblox servers. Server
//! `Script`s run on `rbx_lua`'s engine ([`rbx_lua::server::ServerVm`]),
//! parts move under Rapier ([`physics::World`]), and [`Simulation`] steps
//! both through Roblox's frame, 60 frames a second.
//!
//! The plan, the sources and what's left: `docs/research/local-run/`.

pub mod animation;
pub mod character;
pub mod conformance;
mod engine;
pub mod gui;
pub mod humanoid;
pub mod input;
pub mod load;
pub mod ownership;
pub mod physics;
pub mod players;
mod scripts;
pub mod session;
mod sim;
mod sound;
mod spatial;
mod teams;
mod tools;

pub use input::{Input, MouseBehavior};
pub use players::Avatars;
pub use rbx_lua::{LineKind, Output};
pub use session::{Peer, Session};
pub use sim::{Role, Simulation, FRAME_HZ};
