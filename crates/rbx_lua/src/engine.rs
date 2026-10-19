//! The Roblox runtime both VMs share — the command bar's [`crate::Runtime`]
//! and each plugin's: the scheduler behind `task`, signals and instance
//! events, `require` of `ModuleScript`s, JSON and `CollectionService`.
//!
//! A VM carries one [`Engine`] as app data. What differs between the two
//! hosts (where time comes from, where output goes, which root instances
//! are the host's own rather than the place's) is behind [`Host`].

pub(crate) mod changes;
pub(crate) mod collection;
pub(crate) mod events;
pub(crate) mod json;
pub(crate) mod require;
pub(crate) mod scheduler;
pub(crate) mod signal;

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Thread};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::output::LineKind;
use require::Modules;
use scheduler::Scheduler;
use signal::Signals;

pub(crate) trait Host {
    /// Seconds on the VM's clock: `time()` and every timer.
    fn now(&self) -> f64;

    /// Told the time `scheduler::tick` runs at.
    fn set_now(&self, _now: f64) {}

    /// A fresh id, for an await and for anything else the host numbers.
    fn next_id(&self) -> u32;

    fn output(&self, kind: LineKind, text: String);

    /// `thread` raised `error` while the scheduler resumed it outside the
    /// runner (a script's main chunk, a raw coroutine that waited).
    fn thread_failed(&self, _thread: &Thread, error: mlua::Error) {
        self.output(LineKind::Error, error.to_string());
    }

    /// Whether a root of the DOM belongs to the host rather than to the
    /// place (a plugin's parked instances): `DescendantAdded` on `game` and
    /// `CollectionService:GetTagged` leave those out.
    fn is_parked(&self, _root: Ref) -> bool {
        false
    }
}

pub(crate) struct Engine {
    pub(crate) ctx: Ctx,
    pub(crate) host: Rc<dyn Host>,
    pub(crate) scheduler: RefCell<Scheduler>,
    pub(crate) signals: RefCell<Signals>,
    pub(crate) modules: RefCell<Modules>,
}

impl Engine {
    pub(crate) fn new(ctx: Ctx, host: Rc<dyn Host>) -> Rc<Self> {
        Rc::new(Engine {
            ctx,
            host,
            scheduler: RefCell::default(),
            signals: RefCell::default(),
            modules: RefCell::default(),
        })
    }
}

/// The [`Engine`] of the VM running this callback; every binding that
/// needs one is only installed by [`install`], so a missing one is a bug.
pub(crate) fn engine(lua: &Lua) -> Rc<Engine> {
    lua.app_data_ref::<Rc<Engine>>()
        .expect("engine binding called in a VM without an engine")
        .clone()
}

/// Makes `engine` the VM's and installs the scheduler's globals. Call
/// before sandboxing.
pub(crate) fn install(lua: &Lua, engine: Rc<Engine>) -> mlua::Result<()> {
    lua.set_app_data(engine);
    scheduler::install(lua)
}
