//! A game server's scripts, for `rbx_sim`'s local Run: `Script`s run in
//! their own environments on the shared engine (scheduler, signals,
//! `require`), on a clock the simulation drives frame by frame.
//!
//! The VM knows nothing about physics. It fires what the simulation tells
//! it to (`RunService` steps, `Touched`), hands the DOM changes scripts
//! made back to the simulation, and asks a [`Physics`] for what only the
//! simulation knows (masses) or can do (impulses).

mod hooks;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::chunk::Compiler;
use mlua::{IntoLua, Lua, MultiValue, Table, VmState};
use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::ctx::Ctx;
use crate::engine::changes::{fire_changes, first_number};
use crate::engine::events::instance_key;
use crate::engine::signal::{self, Owner, SignalKey};
use crate::engine::{self, engine, scheduler, Engine, Host};
use crate::hooks::HookSlot;
use crate::instance::tree::full_name;
use crate::instance::LuaInstance;
use crate::output::{LineKind, Output};
use crate::plugin::api::Arg;
use crate::runtime::{install_globals, install_require, install_shared};
use crate::LuaError;

pub use hooks::CollisionGroups;

/// Roblox's message for a script that ran too long without yielding.
const TIMEOUT: &str = "Script timeout: exhausted allowed execution time";

/// How many times one call fires the changes event handlers made in turn.
const ROUNDS: usize = 8;

/// What the VM asks the physics simulation.
pub trait Physics {
    /// `BasePart:GetMass()` / `BasePart.Mass`.
    fn mass(&self, part: Ref) -> Option<f64>;
    /// `BasePart.AssemblyMass`.
    fn assembly_mass(&self, part: Ref) -> Option<f64>;
    /// `ApplyImpulse` (`angular: false`) / `ApplyAngularImpulse`, applied
    /// at the next physics step.
    fn apply_impulse(&self, part: Ref, impulse: [f64; 3], angular: bool);
}

/// Something the simulation tells the scripts happened.
#[derive(Debug, Clone)]
pub enum Event {
    /// A `RunService` signal with its number arguments.
    RunService(&'static str, Vec<f64>),
    /// `part.Touched(other)`.
    Touched { part: Ref, other: Ref },
    /// `part.TouchEnded(other)`.
    TouchEnded { part: Ref, other: Ref },
}

struct ServerHost {
    output: Rc<RefCell<Output>>,
    now: Cell<f64>,
    next_id: Cell<u32>,
}

impl Host for ServerHost {
    fn now(&self) -> f64 {
        self.now.get()
    }

    fn set_now(&self, now: f64) {
        self.now.set(now);
    }

    fn next_id(&self) -> u32 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }

    fn output(&self, kind: LineKind, text: String) {
        self.output.borrow_mut().push_text(kind, text);
    }
}

/// The server's Luau VM over a DOM shared with the simulation.
pub struct ServerVm {
    lua: Lua,
    dom: Rc<RefCell<WeakDom>>,
    output: Rc<RefCell<Output>>,
    host: Rc<ServerHost>,
    groups: Rc<RefCell<CollisionGroups>>,
    /// Scripts already started, so a script is never run twice.
    started: RefCell<BTreeSet<Ref>>,
    budget: Duration,
}

impl ServerVm {
    pub fn new(
        dom: Rc<RefCell<WeakDom>>,
        database: ReflectionDatabase,
        physics: Rc<dyn Physics>,
    ) -> Result<Self, LuaError> {
        let output = Rc::new(RefCell::new(Output::default()));
        let ctx = Ctx::new(dom.clone(), Rc::new(database), output.clone());
        let host = Rc::new(ServerHost {
            output: output.clone(),
            now: Cell::new(0.0),
            next_id: Cell::new(0),
        });
        let groups = Rc::new(RefCell::new(CollisionGroups::default()));
        let lua = Lua::new();
        // See `Runtime::build`: DOM-backed globals must not be import-cached.
        lua.set_compiler(Compiler::new().set_mutable_globals(["game", "workspace", "Workspace"]));
        lua.set_app_data(HookSlot(Rc::new(hooks::ServerHooks {
            physics,
            groups: groups.clone(),
        })));
        install_globals(&lua, &ctx)?;
        lua.globals().set(
            "printidentity",
            crate::runtime::printidentity(&lua, &ctx, 2)?,
        )?;
        engine::install(&lua, Engine::new(ctx, host.clone()))?;
        install_require(&lua)?;
        lua.sandbox(true)?;
        install_shared(&lua)?;
        Ok(ServerVm {
            lua,
            dom,
            output,
            host,
            groups,
            started: RefCell::default(),
            budget: Duration::from_secs(10),
        })
    }

    /// The collision groups `PhysicsService` registered.
    pub fn collision_groups(&self) -> Rc<RefCell<CollisionGroups>> {
        self.groups.clone()
    }

    /// The game time `time()`, timers and `Stepped` read.
    pub fn set_time(&self, seconds: f64) {
        self.host.now.set(seconds);
    }

    pub fn time(&self) -> f64 {
        self.host.now.get()
    }

    /// Queues `script`'s body to run at the next [`ServerVm::resume`], in
    /// an environment of its own whose `script` is it. A script is started
    /// once; a second call does nothing.
    pub fn start_script(&self, script: Ref) -> Result<(), LuaError> {
        if !self.started.borrow_mut().insert(script) {
            return Ok(());
        }
        let ctx = engine(&self.lua).ctx.clone();
        let (source, name) = {
            let dom = ctx.dom();
            let Some(instance) = dom.get(script) else {
                return Ok(());
            };
            let source = match instance.properties().get("Source") {
                Some(Variant::String(text)) => text.clone(),
                _ => String::new(),
            };
            let name = full_name(&dom, script).unwrap_or_else(|| instance.name().to_string());
            (source, name)
        };
        let this = LuaInstance::new(script, ctx).into_lua(&self.lua)?;
        let scope: Table = self.lua.create_table()?;
        scope.set("script", this.clone())?;
        scope.set("require", crate::engine::require::require_for(&self.lua, this)?)?;
        let meta = self.lua.create_table()?;
        meta.set("__index", self.lua.globals())?;
        scope.set_metatable(Some(meta))?;
        let chunk = self
            .lua
            .load(source)
            .set_name(format!("={name}"))
            .set_environment(scope)
            .into_function();
        match chunk {
            Ok(chunk) => {
                let main = self.lua.create_thread(chunk)?;
                engine(&self.lua)
                    .scheduler
                    .borrow_mut()
                    .deferred
                    .push_back((main, MultiValue::new()));
            }
            // Roblox reports a script that does not compile and moves on.
            Err(error) => self.host.output(LineKind::Error, error.to_string()),
        }
        Ok(())
    }

    /// Runs the threads whose wait is over at the current time and the
    /// deferred queue, then the instance events their changes call for.
    /// Returns every DOM change scripts made, drained from the log.
    pub fn resume(&mut self) -> Vec<Change> {
        let now = self.host.now.get();
        self.guarded(|lua| scheduler::tick(lua, now))
    }

    /// Fires `event` to the scripts (handlers run now), then the instance
    /// events their changes call for; returns the DOM changes made.
    pub fn fire(&mut self, event: Event) -> Vec<Change> {
        self.guarded(|lua| match &event {
            Event::RunService(name, args) => {
                let args: Vec<Arg> = args.iter().copied().map(Arg::Number).collect();
                signal::fire(lua, &SignalKey::new(Owner::Service("RunService"), *name), &args)
            }
            Event::Touched { part, other } => {
                signal::fire(lua, &instance_key(*part, "Touched"), &[Arg::Instance(*other)])
            }
            Event::TouchEnded { part, other } => signal::fire(
                lua,
                &instance_key(*part, "TouchEnded"),
                &[Arg::Instance(*other)],
            ),
        })
    }

    /// Whether anything listens to `part`'s `Touched` or `TouchEnded`.
    pub fn listens_to_touch(&self, part: Ref) -> bool {
        let engine = engine(&self.lua);
        let signals = engine.signals.borrow();
        signals.has(&instance_key(part, "Touched")) || signals.has(&instance_key(part, "TouchEnded"))
    }

    /// When the next timer is due, `None` when nothing waits on time.
    pub fn next_due(&self) -> Option<f64> {
        engine(&self.lua).scheduler.borrow().next_due(self.host.now.get())
    }

    /// What scripts printed since the last call.
    pub fn take_output(&self) -> Output {
        std::mem::take(&mut *self.output.borrow_mut())
    }

    /// Destroys `root` the way `Instance:Destroy` does (`Destroying`
    /// fires, scripts holding it still read its last name and class),
    /// for the engine's own removals such as `FallenPartsDestroyHeight`.
    pub fn destroy(&mut self, root: Ref) -> Vec<Change> {
        self.guarded(|lua| {
            let ctx = engine(lua).ctx.clone();
            if ctx.dom().get(root).is_none() {
                return Ok(());
            }
            crate::engine::changes::destroying(lua, root)?;
            let mut dom = ctx.dom_mut();
            ctx.bury(&dom, root);
            dom.remove(root);
            Ok(())
        })
    }

    /// Runs `f` under the script timeout, then fires the changes made,
    /// round after round, and drains them from the log.
    fn guarded(&mut self, f: impl FnOnce(&Lua) -> mlua::Result<()>) -> Vec<Change> {
        let deadline = Instant::now() + self.budget;
        self.lua.set_interrupt(move |_| {
            if Instant::now() >= deadline {
                Err(mlua::Error::runtime(TIMEOUT))
            } else {
                Ok(VmState::Continue)
            }
        });
        let mut seen = self.dom.borrow().changes().len();
        let mut ran = f(&self.lua);
        for _ in 0..ROUNDS {
            if ran.is_err() {
                break;
            }
            let (fresh, first) = {
                let dom = self.dom.borrow();
                let fresh = dom.changes()[seen..].to_vec();
                seen += fresh.len();
                let first = first_number(&dom, fresh.len());
                (fresh, first)
            };
            if fresh.is_empty() {
                break;
            }
            ran = fire_changes(&self.lua, &fresh, Some(first));
        }
        engine(&self.lua).signals.borrow_mut().settle();
        self.lua.remove_interrupt();
        if let Err(error) = ran {
            self.host.output(LineKind::Error, error.to_string());
        }
        self.dom.borrow_mut().take_changes()
    }
}
