//! Synchronous Luau scripting over a `WeakDom`, in the spirit of Studio's
//! command bar.
//!
//! [`Runtime`] owns the DOM while a script runs and exposes it to Luau through
//! `game`, `workspace`, `Instance`, `Enum` and a first batch of datatypes. There
//! is no scheduler: scripts run to completion, with no events and no yielding.

mod ctx;
mod datatypes;
mod debugger;
mod defaults;
mod game;
mod instance;
mod lua_enum;
mod not_creatable;
mod property;
mod runtime;

pub use debugger::{Breakpoint, Frame, Paused, Resume, Variable, STOPPED};
pub use mlua::Error as LuaError;
pub use runtime::{Output, Runtime};
