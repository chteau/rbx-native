//! A game server's scripts, for `rbx_sim`'s local Run: `Script`s run in
//! their own environments on the shared engine (scheduler, signals,
//! `require`), on a clock the simulation drives frame by frame.
//!
//! The VM knows nothing about physics. It fires what the simulation tells
//! it to (`RunService` steps, `Touched`), hands the DOM changes scripts
//! made back to the simulation, and asks a [`Physics`] for what only the
//! simulation knows (masses) or can do (impulses).

mod chat;
pub(crate) mod editable_image;
mod hooks;
mod input;
mod meshes;
mod pathfinding;
mod query;
mod remote;
mod services;
mod styles;
mod terrain;
#[cfg(test)]
mod tests;
mod tween;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::chunk::Compiler;
use mlua::{Function, IntoLua, Lua, MultiValue, Table, VmState};
use rbx_dom::{CFrameData, Change, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::ctx::Ctx;
use crate::engine::changes::{fire_changes, first_number};
use crate::engine::events::{self, instance_key};
use crate::engine::signal::{self, deferred_signals, Owner, SignalKey};
use crate::engine::{self, engine, scheduler, Engine, Host};
use crate::hooks::HookSlot;
use crate::instance::tree::full_name;
use crate::instance::LuaInstance;
use crate::output::{LineKind, Output};
pub use crate::plugin::api::Arg;
use crate::runtime::{install_globals, install_require, install_shared};
use crate::LuaError;

pub use chat::ChatLine;
pub use hooks::CollisionGroups;
pub use query::{QueryFilter, QueryHit, QueryShape};
pub use remote::{Outgoing, Packet, To, Wire};
pub use services::{
    set_text_measure, Arrival, Device, Notice, PurchasePrompt, Services, TeleportRequest,
    TextMeasure,
};
pub use terrain::{TerrainOp, TerrainReply};

/// Roblox's message for a script that ran too long without yielding.
const TIMEOUT: &str = "Script timeout: exhausted allowed execution time";

const LEGACY_WAIT: &str = r#"
local wait = ...
return function(seconds)
    return wait(math.max(tonumber(seconds) or 0, 0.03))
end
"#;

/// How many times one call fires the changes event handlers made in turn.
const ROUNDS: usize = 8;

/// The registry list of `game:BindToClose` functions (and, once closing,
/// its `pending` count).
pub(crate) const BIND_TO_CLOSE: &str = "rbxn.bind_to_close";

/// Starts each BindToClose function on its own thread; an error is
/// reported once it has counted as returned.
const CLOSE: &str = r#"
return function(functions)
	functions.pending = #functions
	for _, f in ipairs(functions) do
		task.spawn(function()
			local ok, message = pcall(f)
			functions.pending -= 1
			if not ok then
				error(message, 0)
			end
		end)
	end
end
"#;

/// What the VM asks the physics simulation.
pub trait Physics {
    /// `BasePart:GetMass()` / `BasePart.Mass`.
    fn mass(&self, part: Ref) -> Option<f64>;
    /// `BasePart.AssemblyMass`.
    fn assembly_mass(&self, part: Ref) -> Option<f64>;
    /// `ApplyImpulse` (`angular: false`) / `ApplyAngularImpulse`, applied
    /// at the next physics step.
    fn apply_impulse(&self, part: Ref, impulse: [f64; 3], angular: bool);

    /// `Workspace:GetNumAwakeParts()`: the unanchored parts not asleep.
    fn awake_parts(&self) -> usize {
        0
    }

    /// A `MeshPart` made at runtime: its geometry, `vertices` centred on
    /// the part's origin, for collision.
    fn register_mesh(&self, _part: Ref, _vertices: Vec<[f32; 3]>, _triangles: Vec<[u32; 3]>) {}

    /// A `Terrain` voxel edit or read.
    fn terrain(&self, _op: TerrainOp) -> Result<TerrainReply, String> {
        Err("Terrain is not simulated here".to_owned())
    }

    /// `WorldRoot:Raycast`: the nearest hit along `direction` (its whole
    /// length) from `origin`.
    fn raycast(
        &self,
        _origin: [f64; 3],
        _direction: [f64; 3],
        _filter: &QueryFilter,
    ) -> Option<QueryHit> {
        None
    }

    /// `Blockcast`, `Spherecast`, `Shapecast`: `shape` moved from `from`
    /// along `direction`.
    fn shapecast(
        &self,
        _shape: &QueryShape,
        _from: &CFrameData,
        _direction: [f64; 3],
        _filter: &QueryFilter,
    ) -> Option<QueryHit> {
        None
    }

    /// `GetPartBoundsIn*` (`exact: false`) and `GetPartsInPart`
    /// (`exact: true`).
    fn overlap(
        &self,
        _shape: &QueryShape,
        _at: &CFrameData,
        _exact: bool,
        _filter: &QueryFilter,
    ) -> Vec<Ref> {
        Vec::new()
    }

    /// Whether the simulation answers `class:key(...)` (a Humanoid's
    /// `MoveTo`, a player's `LoadCharacter`, ...) through [`Physics::call`].
    fn engine_method(&self, _class: &str, _key: &str) -> bool {
        false
    }

    /// Whether `class.key` is a signal the simulation fires
    /// ([`Event::Signal`] or [`CallReply::fire`]).
    fn engine_signal(&self, _class: &str, _key: &str) -> bool {
        false
    }

    /// `referent:method(args)` for a method [`Physics::engine_method`]
    /// claimed (`args` without the instance itself).
    fn call(
        &self,
        _referent: Ref,
        _class: &str,
        method: &str,
        _args: Vec<Arg>,
    ) -> Result<CallReply, String> {
        Err(format!("{method} is not a valid member"))
    }

    /// An engine property read the simulation answers live (a player's
    /// `Character`, `Players.LocalPlayer`), `None` for the DOM's value.
    /// A method whose first argument names a signal it returns, as
    /// `<method>:<argument>` (`AnimationTrack:GetMarkerReachedSignal`).
    fn signal_method(&self, _class: &str, _key: &str) -> bool {
        false
    }

    fn engine_property(&self, _referent: Ref, _class: &str, _key: &str) -> Option<Arg> {
        None
    }

    /// A script's write of `class.key` the simulation changes on the way in
    /// (a Humanoid's `Health` above `MaxHealth`): the value to store
    /// instead, `None` to store it as given.
    fn engine_write(&self, _referent: Ref, _class: &str, _key: &str, _value: &Arg) -> Option<Arg> {
        None
    }

    /// `BasePart:GetNetworkOwner()`: the owning player, `None` for the
    /// server; `Err` with Roblox's message for an anchored assembly.
    fn network_owner(&self, _part: Ref) -> Result<Option<Ref>, String> {
        Ok(None)
    }

    /// `SetNetworkOwner(player)` (`None`: the server) and
    /// `SetNetworkOwnershipAuto()` (`auto`).
    fn set_network_owner(
        &self,
        _part: Ref,
        _owner: Option<Ref>,
        _auto: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    /// `GetNetworkOwnershipAuto()`.
    fn network_ownership_auto(&self, _part: Ref) -> Result<bool, String> {
        Ok(true)
    }

    /// `child` moved from `old` to `new`, as its events fire: the signals
    /// the engine adds, fired just before its `AncestryChanged`
    /// (`after: false`; a tool's `Unequipped`) or after its
    /// `Changed:Parent` (`after: true`; a tool's `Equipped`).
    fn reparented(
        &self,
        _child: Ref,
        _old: Option<Ref>,
        _new: Option<Ref>,
        _after: bool,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        Vec::new()
    }

    /// A script wrote `class.key`: the signals that fire for it before the
    /// next line (a team's `PlayerAdded` for a player's `TeamColor`).
    fn property_written(
        &self,
        _referent: Ref,
        _class: &str,
        _key: &str,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        Vec::new()
    }
}

/// What a [`Physics::call`] returns, and the signals it fires before it
/// returns, in order (`LoadCharacter`'s `CharacterRemoving`,
/// `CharacterAdded`, ... run before the call's next line).
#[derive(Debug, Default)]
pub struct CallReply {
    pub values: Vec<Arg>,
    pub fire: Vec<(Ref, String, Vec<Arg>)>,
    /// A method to call next, once `fire`'s handlers have run, with the
    /// same arguments: a call that must let scripts see a state between two
    /// steps (`LoadCharacter`'s `CharacterRemoving` before the old
    /// character goes).
    pub and_then: Option<String>,
}

impl CallReply {
    pub fn value(value: Arg) -> Self {
        CallReply {
            values: vec![value],
            ..CallReply::default()
        }
    }
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
    /// `owner[name]` fired with `args` (a Humanoid's `StateChanged`, ...).
    Signal {
        owner: Ref,
        name: String,
        args: Vec<Arg>,
    },
}

struct ServerHost {
    output: Rc<RefCell<Output>>,
    now: Cell<f64>,
    next_id: Cell<u32>,
    physics: Rc<dyn Physics>,
}

impl Host for ServerHost {
    fn now(&self) -> f64 {
        self.now.get()
    }

    fn set_now(&self, now: f64) {
        self.now.set(now);
    }

    fn frame_clock(&self) -> bool {
        true
    }

    /// The SignalBehavior golden: A, B connected, then a `Wait` W, then C,
    /// fire as `C W B A`.
    fn newest_first(&self) -> bool {
        true
    }

    /// The SignalBehavior golden recorded 2026-11 (place version 537):
    /// handlers run after `Fire` and after a property write returns, on a
    /// place that leaves `SignalBehavior` at `Default`.
    fn default_deferred(&self) -> bool {
        true
    }

    fn next_id(&self) -> u32 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }

    fn output(&self, kind: LineKind, text: String) {
        self.output.borrow_mut().push_text(kind, text);
    }

    fn reparented(
        &self,
        child: Ref,
        old: Option<Ref>,
        new: Option<Ref>,
        after: bool,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        self.physics.reparented(child, old, new, after)
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
    /// How much of the DOM's change log has had its events fired.
    fired: Rc<Cell<usize>>,
    client: Rc<Cell<bool>>,
    remotes: remote::Shared,
    chat: chat::Chat,
    loaded: Rc<Cell<bool>>,
    services: Rc<services::Runtime>,
    styles: styles::Shared,
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
            physics: physics.clone(),
        });
        let groups = Rc::new(RefCell::new(CollisionGroups::default()));
        let lua = Lua::new();
        // See `Runtime::build`: DOM-backed globals must not be import-cached.
        lua.set_compiler(Compiler::new().set_mutable_globals(["game", "workspace", "Workspace"]));
        let fired = Rc::new(Cell::new(0));
        let client = Rc::new(Cell::new(false));
        let remotes = remote::Shared::default();
        let chat = chat::Chat {
            remotes: remotes.clone(),
            client: client.clone(),
            log: Rc::default(),
        };
        let loaded = Rc::new(Cell::new(true));
        let services = services::Runtime::new(client.clone(), physics.clone(), remotes.clone());
        let styles = styles::Shared::default();
        lua.set_app_data(HookSlot(Rc::new(hooks::ServerHooks {
            physics,
            groups: groups.clone(),
            fired: fired.clone(),
            mark: Cell::new(usize::MAX),
            client: client.clone(),
            remotes: remotes.clone(),
            chat: chat.clone(),
            loaded: loaded.clone(),
            services: services.clone(),
            styles: styles.clone(),
        })));
        install_globals(&lua, &ctx)?;
        lua.globals().set(
            "printidentity",
            crate::runtime::printidentity(&lua, &ctx, 2)?,
        )?;
        engine::install(&lua, Engine::new(ctx, host.clone()))?;
        install_require(&lua)?;
        // The legacy `wait` never takes less than two 60 Hz frames on a
        // real server (the TaskTiming golden: `wait()` = 2 heartbeats,
        // 0.033 s); `task.wait` has no such floor.
        let legacy: mlua::Function = lua.load(LEGACY_WAIT).set_name("=wait").call(
            lua.globals()
                .get::<Table>("task")?
                .get::<mlua::Function>("wait")?,
        )?;
        lua.globals().set("wait", legacy)?;
        let runtime = services.clone();
        lua.globals().set(
            "UserSettings",
            lua.create_function(move |lua, ()| services::user_settings(lua, &runtime))?,
        )?;
        lua.sandbox(true)?;
        install_shared(&lua)?;
        Ok(ServerVm {
            lua,
            dom,
            output,
            host,
            groups,
            started: RefCell::default(),
            fired,
            client,
            remotes,
            chat,
            loaded,
            services,
            styles,
            budget: Duration::from_secs(10),
        })
    }

    /// How long one resumption of a script may run before it dies (10 s,
    /// Studio's default). Tests raise it: it is the one wall clock a run
    /// reads, and a loaded machine can take that long on a heavy test.
    pub fn set_script_timeout(&mut self, timeout: Duration) {
        self.budget = timeout;
    }

    /// Makes this VM a client's: `RunService:IsClient()` is true and
    /// `IsServer()` false, `RunService:BindToRenderStep`,
    /// `UserInputService` and `ContextActionService` work, and remotes go
    /// to the server.
    pub fn set_client(&self, on: bool) {
        self.client.set(on);
    }

    /// `game:IsLoaded()`. Turning it on fires `game.Loaded`.
    pub fn set_loaded(&mut self, on: bool) -> Vec<Change> {
        let was = self.loaded.replace(on);
        if !on || was {
            return Vec::new();
        }
        self.guarded(|lua| {
            signal::fire(
                lua,
                &SignalKey::new(Owner::Service("DataModel"), "Loaded"),
                &[],
            )
        })
    }

    /// Fires the instance events for DOM changes made from outside (a
    /// replicated change) and returns what their handlers changed.
    pub fn flush_changes(&mut self) -> Vec<Change> {
        self.guarded_from(0, |_| Ok(()))
    }

    /// The class and name a destroyed instance had.
    pub fn tombstone(&self, referent: Ref) -> Option<(String, String)> {
        engine(&self.lua).ctx.tombstone(referent)
    }

    /// The remote packets scripts sent since the last call, in order.
    pub fn take_outgoing(&self) -> Vec<Outgoing> {
        std::mem::take(&mut self.remotes.borrow_mut().outbox)
    }

    /// A packet from the other side: `from` is the player whose client
    /// sent it (`None` from the server). Listeners and callbacks run now;
    /// events for a remote nobody listens to yet wait for a listener.
    pub fn receive(&mut self, from: Option<Ref>, packet: Packet) -> Vec<Change> {
        if matches!(packet, Packet::Chat { .. }) {
            let chat = self.chat.clone();
            return self.guarded(move |lua| chat::receive(lua, &chat, from, packet));
        }
        if matches!(packet, Packet::Drag { .. }) {
            let services = self.services.clone();
            return self.guarded(move |lua| services::receive_drag(lua, &services, from, packet));
        }
        let remotes = self.remotes.clone();
        let client = self.client.get();
        self.guarded(move |lua| remote::receive(lua, &remotes, client, from, packet))
    }

    /// The start of a frame's input: `GetMouseDelta` counts from zero.
    pub fn input_frame(&mut self) -> Vec<Change> {
        self.guarded(input::frame)
    }

    /// Hands one input the engine wrote into `input` (an `InputObject`) to
    /// the game as a client does: the bound actions first (none when the
    /// GUI already `processed` it), then `uis`'s `signal` (`InputBegan`,
    /// `InputChanged` or `InputEnded`) with whether either took it.
    pub fn input(
        &mut self,
        input: Ref,
        uis: Ref,
        signal_name: &str,
        processed: bool,
    ) -> Vec<Change> {
        let services = self.services.clone();
        self.guarded(|lua| {
            let processed = input::dispatch(lua, input, processed)?;
            let object = LuaInstance::new(input, engine(lua).ctx.clone()).into_lua(lua)?;
            services::client_input(lua, &services, object, processed)?;
            signal::fire(
                lua,
                &instance_key(uis, signal_name),
                &[Arg::Instance(input), Arg::Bool(processed)],
            )
        })
    }

    /// A client's render step: the `BindToRenderStep` callbacks in priority
    /// order, then `PreRender` and `RenderStepped`.
    pub fn render_step(&mut self, dt: f64) -> Vec<Change> {
        self.guarded(|lua| {
            input::render(lua, dt)?;
            for name in ["PreRender", "RenderStepped"] {
                signal::fire(
                    lua,
                    &SignalKey::new(Owner::Service("RunService"), name),
                    &[Arg::Number(dt)],
                )?;
            }
            Ok(())
        })
    }

    /// The editor's chat bar: this client's player says `text` in the
    /// general channel, as `TextChannel:SendAsync` would.
    pub fn chat_say(&mut self, text: &str) -> Vec<Change> {
        let chat = self.chat.clone();
        let text = text.to_owned();
        self.guarded(move |lua| chat::say(lua, &chat, &text))
    }

    /// The chat messages this client showed since the last call.
    pub fn take_chat(&self) -> Vec<ChatLine> {
        std::mem::take(&mut self.chat.log.borrow_mut())
    }

    /// Steps every playing tween `dt` seconds (their writes and
    /// `Completed`).
    pub fn tween_step(&mut self, dt: f64) -> Vec<Change> {
        self.guarded(|lua| tween::step(lua, dt))
    }

    /// Where this server's DataStores live, whether Studio's API access is
    /// on, which servers it shares them and messages with.
    pub fn set_services(&self, config: Services) {
        self.services.configure(config);
    }

    /// The purchase prompts scripts asked for since the last call.
    pub fn take_prompts(&self) -> Vec<PurchasePrompt> {
        self.services.take_prompts()
    }

    /// The teleports into reloadable places scripts asked for since the
    /// last call.
    pub fn take_teleports(&self) -> Vec<TeleportRequest> {
        self.services.take_teleports()
    }

    /// The toasts scripts asked the core GUI for since the last call.
    pub fn take_notices(&self) -> Vec<Notice> {
        self.services.take_notices()
    }

    /// The test purchase dialog for `prompt` closed (`bought` when the
    /// player confirmed): the Prompt*Finished events, ownership, and on a
    /// server ProcessReceipt for a developer product.
    pub fn purchase_finished(&mut self, prompt: &PurchasePrompt, bought: bool) -> Vec<Change> {
        let runtime = self.services.clone();
        let prompt = prompt.clone();
        self.guarded(move |lua| services::finished(lua, &runtime, &prompt, bought))
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
        scope.set(
            "require",
            crate::engine::require::require_for(&self.lua, this)?,
        )?;
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
                let engine = engine(&self.lua);
                let mut scheduler = engine.scheduler.borrow_mut();
                scheduler.owners.push((main.clone(), script));
                scheduler.deferred.push_back((main, MultiValue::new()));
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
        let remotes = self.remotes.clone();
        let client = self.client.get();
        let runtime = self.services.clone();
        let styles = self.styles.clone();
        self.guarded(move |lua| {
            remote::flush(lua, &remotes, client)?;
            services::poll(lua, &runtime)?;
            styles::poll(lua, &styles)?;
            scheduler::tick(lua, now)
        })
    }

    /// Fires `event` to the scripts (handlers run now), then the instance
    /// events their changes call for; returns the DOM changes made.
    pub fn fire(&mut self, event: Event) -> Vec<Change> {
        self.guarded(|lua| match &event {
            Event::RunService(name, args) => {
                let args: Vec<Arg> = args.iter().copied().map(Arg::Number).collect();
                signal::fire(
                    lua,
                    &SignalKey::new(Owner::Service("RunService"), *name),
                    &args,
                )
            }
            Event::Touched { part, other } => signal::fire(
                lua,
                &instance_key(*part, "Touched"),
                &[Arg::Instance(*other)],
            ),
            Event::TouchEnded { part, other } => signal::fire(
                lua,
                &instance_key(*part, "TouchEnded"),
                &[Arg::Instance(*other)],
            ),
            Event::Signal { owner, name, args } => {
                signal::fire(lua, &instance_key(*owner, name.as_str()), args)
            }
        })
    }

    /// The parts something listens to the `Touched` or `TouchEnded` of.
    pub fn touch_listeners(&self) -> std::collections::HashSet<Ref> {
        let engine = engine(&self.lua);
        let signals = engine.signals.borrow();
        signals
            .connections
            .iter()
            .filter(|(key, list)| {
                !list.is_empty() && matches!(key.event.as_str(), "Touched" | "TouchEnded")
            })
            .filter_map(|(key, _)| match key.owner {
                signal::Owner::Instance(part) => Some(part),
                _ => None,
            })
            .collect()
    }

    /// When the next timer is due, `None` when nothing waits on time.
    pub fn next_due(&self) -> Option<f64> {
        engine(&self.lua)
            .scheduler
            .borrow()
            .next_due(self.host.now.get())
    }

    /// What scripts printed since the last call.
    pub fn take_output(&self) -> Output {
        std::mem::take(&mut *self.output.borrow_mut())
    }

    /// Destroys `root` the way `Instance:Destroy` does (`Destroying`
    /// fires, scripts holding it still read its last name and class),
    /// for the engine's own removals such as `FallenPartsDestroyHeight`.
    /// The server shuts down: each `game:BindToClose` function starts on a
    /// thread of its own. [`ServerVm::closed`] says when all have returned.
    pub fn close(&mut self) -> Vec<Change> {
        self.guarded(|lua| {
            let functions = events::registry_table(lua, BIND_TO_CLOSE)?;
            let run: Function = lua.load(CLOSE).set_name("=BindToClose").eval()?;
            run.call::<()>(functions)
        })
    }

    /// Whether every `BindToClose` function started by
    /// [`ServerVm::close`] has returned.
    pub fn closed(&self) -> bool {
        events::registry_table(&self.lua, BIND_TO_CLOSE)
            .and_then(|t| t.get::<i64>("pending"))
            .map_or(true, |n| n <= 0)
    }

    pub fn destroy(&mut self, root: Ref) -> Vec<Change> {
        self.guarded(|lua| {
            let ctx = engine(lua).ctx.clone();
            crate::engine::changes::destroy(lua, &ctx, root)
        })
    }

    /// Runs `f` under the script timeout, then fires the changes made,
    /// round after round, and drains them from the log.
    fn guarded(&mut self, f: impl FnOnce(&Lua) -> mlua::Result<()>) -> Vec<Change> {
        let from = self.dom.borrow().changes().len();
        self.guarded_from(from, f)
    }

    /// [`ServerVm::guarded`], firing the instance events of the logged
    /// changes past `from` too.
    fn guarded_from(
        &mut self,
        from: usize,
        f: impl FnOnce(&Lua) -> mlua::Result<()>,
    ) -> Vec<Change> {
        let deadline = crate::engine::scheduler::Deadline::arm(&self.lua, self.budget);
        self.lua.set_interrupt(move |_| {
            if Instant::now() >= deadline.get() {
                Err(mlua::Error::runtime(TIMEOUT))
            } else {
                Ok(VmState::Continue)
            }
        });
        self.fired.set(from);
        let mut ran = f(&self.lua);
        for _ in 0..ROUNDS {
            match ran {
                Ok(()) => {}
                Err(_) => break,
            }
            // Deferred signals: what was queued runs at the end of this
            // resumption point, ahead of the instance events of its writes.
            if deferred_signals(&self.lua) {
                if let Err(error) = scheduler::run_deferred(&self.lua) {
                    ran = Err(error);
                    break;
                }
            }
            match fire_fresh(&self.lua, &self.fired) {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => ran = Err(error),
            }
        }
        engine(&self.lua).signals.borrow_mut().settle();
        self.lua.remove_interrupt();
        if let Err(error) = ran {
            self.host.output(LineKind::Error, error.to_string());
        }
        self.fired.set(0);
        self.dom.borrow_mut().take_changes()
    }
}

/// Fires the instance events for the changes logged past `fired`, and
/// moves `fired` past them first, so a handler's own writes fire after.
/// `false` when there was nothing new.
fn fire_fresh(lua: &Lua, fired: &Cell<usize>) -> mlua::Result<bool> {
    let ctx = engine(lua).ctx.clone();
    let (fresh, first) = {
        let dom = ctx.dom();
        let all = dom.changes();
        let fresh = all[fired.get().min(all.len())..].to_vec();
        fired.set(all.len());
        let first = first_number(&dom, fresh.len());
        (fresh, first)
    };
    if fresh.is_empty() {
        return Ok(false);
    }
    fire_changes(lua, &fresh, Some(first))?;
    Ok(true)
}
