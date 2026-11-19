//! Local Run: a place simulated headless, with no Roblox servers. Server
//! `Script`s run on `rbx_lua`'s engine ([`rbx_lua::server::ServerVm`]),
//! parts move under Rapier ([`physics::World`]), and [`Simulation`] steps
//! both through Roblox's frame, 60 frames a second.
//!
//! The plan, the sources and what's left: `docs/research/local-run/`.

pub mod conformance;
pub mod load;
pub mod physics;
mod scripts;
mod sim;
mod spatial;

pub use rbx_lua::{LineKind, Output};
pub use sim::{Simulation, FRAME_HZ};
