//! The frame loop: scripts and physics stepped together.

use std::cell::{Ref as CellRef, RefCell};
use std::rc::Rc;

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Event, ServerVm};
use rbx_lua::Output;
use rbx_reflection::ReflectionDatabase;

use crate::animation::TrackOp;
use crate::humanoid::State;
use crate::input::{Input, Inputs, MouseBehavior};
use crate::physics::{Bridge, Drive, Shared, Touch, World, PHYSICS_HZ};
use crate::players::{Avatars, Due};
use crate::scripts;

/// Which side of the game a simulation plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The server: server scripts, every player's character.
    Server,
    /// Server and client in one VM (both kinds of script run), with no
    /// replication: what the coverage probe uses.
    Solo,
    /// One player's client, fed by a [`crate::Session`]'s server.
    Client,
}

/// Frames a second, Roblox's server rate.
pub const FRAME_HZ: f64 = 60.0;

const DEFAULT_GRAVITY: f64 = 196.2;
const DEFAULT_FALLEN_HEIGHT: f64 = -500.0;

/// Property writes that change what physics is built from.
const STRUCTURAL: &[&str] = &[
    "Size",
    "size",
    "Shape",
    "shape",
    "Anchored",
    "CanCollide",
    "CanTouch",
    "Massless",
    "Material",
    "CustomPhysicalProperties",
    "CollisionGroup",
    "CollisionGroupId",
    "Part0",
    "Part1",
    "Part0Internal",
    "Part1Internal",
    "Enabled",
];

/// A place running headless.
pub struct Simulation {
    dom: Rc<RefCell<WeakDom>>,
    db: &'static ReflectionDatabase,
    vm: ServerVm,
    world: World,
    shared: Rc<RefCell<Shared>>,
    frame: u64,
    dirty: bool,
    output: Output,
    /// Changes kept for [`Simulation::take_view_changes`], once watched.
    view_changes: Option<Vec<Change>>,
    /// Play (local): the local client's input, `None` for a server-only run.
    inputs: Option<Inputs>,
    role: Role,
    /// Logged changes with their DOM mutation number, for a session to
    /// replicate; `None` outside one.
    stream: Option<Vec<(u64, Change)>>,
}

impl Simulation {
    pub fn new(dom: WeakDom) -> Result<Self, String> {
        let db = ReflectionDatabase::shared();
        let dom = Rc::new(RefCell::new(dom));
        dom.borrow_mut().take_changes();
        let shared = Rc::new(RefCell::new(Shared::default()));
        let vm = ServerVm::new(dom.clone(), db.clone(), Rc::new(Bridge(shared.clone())))
            .map_err(|err| err.to_string())?;
        shared
            .borrow_mut()
            .connect(dom.clone(), db, vm.collision_groups());
        Ok(Simulation {
            dom,
            db,
            vm,
            world: World::new(shared.clone()),
            shared,
            frame: 0,
            dirty: true,
            output: Output::default(),
            view_changes: None,
            inputs: None,
            role: Role::Server,
            stream: None,
        })
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// Server and client in one VM, with no replication: both kinds of
    /// script run and the client services exist. Call before
    /// [`Simulation::start`].
    pub fn set_client(&mut self) {
        self.role = Role::Solo;
        self.client_services();
    }

    /// Makes this the client of local player number `index`: client
    /// scripts only, its own camera and input, `game:IsLoaded()` false
    /// until [`Simulation::set_loaded`], and instances it makes numbered
    /// apart from the server's.
    pub(crate) fn make_client(&mut self, index: usize, player: Ref) {
        self.role = Role::Client;
        {
            let mut shared = self.shared.borrow_mut();
            shared.players.client = true;
            shared.players.local = Some(player);
            shared.ownership.client = true;
            shared.animations.record = true;
        }
        self.dom
            .borrow_mut()
            .raise_next_ref(crate::ownership::client_refs(index));
        self.vm.set_loaded(false);
        self.client_services();
    }

    /// The server of a session: changes and track operations are kept
    /// for replication.
    pub(crate) fn make_server(&mut self) {
        self.stream = Some(Vec::new());
        self.shared.borrow_mut().animations.record = true;
    }

    fn client_services(&mut self) {
        self.vm.set_client(true);
        {
            let mut dom = self.dom.borrow_mut();
            crate::input::client_camera(&mut dom, self.db);
            let uis = crate::input::service(&mut dom, "UserInputService");
            // The docs' defaults, which the reflection database lacks.
            for (key, value) in [
                ("MouseDeltaSensitivity", Variant::Float32(1.0)),
                ("MouseIconEnabled", Variant::Bool(true)),
            ] {
                if dom
                    .get(uis)
                    .is_some_and(|i| !i.properties().contains_key(key))
                {
                    let _ = dom.set_property(uis, key, value);
                }
            }
            crate::input::service(&mut dom, "ContextActionService");
        }
        self.inputs = Some(Inputs::default());
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
    }

    /// Queues what the player did, for the next frame.
    pub fn input(&mut self, input: Input) {
        if let Some(inputs) = &mut self.inputs {
            inputs.pending.push(input);
        }
    }

    /// What `UserInputService.MouseBehavior` asks of the mouse.
    pub fn mouse_behavior(&self) -> MouseBehavior {
        crate::input::mouse_behavior(&self.dom.borrow())
    }

    /// Where the client's camera is (`Workspace.CurrentCamera`'s `CFrame`)
    /// and its field of view in degrees.
    pub fn camera(&self) -> Option<(rbx_dom::CFrameData, f32)> {
        crate::input::camera(&self.dom.borrow())
    }

    /// The client's part of a frame, after the server's: the frame's input
    /// to the game, then the render step (where the camera scripts move
    /// the camera). Roblox runs it ahead of the next frame's simulation;
    /// the order is the same.
    fn client_frame(&mut self, dt: f64) {
        let Some(mut inputs) = self.inputs.take() else {
            return;
        };
        let changes = self.vm.input_frame();
        self.apply(changes);
        let uis = crate::input::service(&mut self.dom.borrow_mut(), "UserInputService");
        for input in std::mem::take(&mut inputs.pending) {
            let written = inputs.write(&mut self.dom.borrow_mut(), self.db, &input);
            let Some((object, signal)) = written else {
                continue;
            };
            let changes = self.vm.input(object, uis, signal);
            self.apply(changes);
            if matches!(&input, Input::Key { code, down: true } if code == "Space") {
                let changes = self.vm.fire(Event::Signal {
                    owner: uis,
                    name: "JumpRequest".into(),
                    args: Vec::new(),
                });
                self.apply(changes);
            }
        }
        self.inputs = Some(inputs);
        let changes = self.vm.render_step(dt);
        self.apply(changes);
        self.collect_output();
    }

    /// Puts a player in the game as `name`, wearing `template` (a DOM
    /// whose one root is the model) or the default character:
    /// `PlayerAdded` and (with `CharacterAutoLoads`) its first character
    /// on the next frame.
    pub fn join(&mut self, name: &str, user_id: i64, template: Option<WeakDom>) -> Option<Ref> {
        let player = {
            let mut dom = self.dom.borrow_mut();
            self.shared
                .borrow_mut()
                .players
                .join(&mut dom, self.db, name, user_id, template)
        };
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        player
    }

    /// The character for players who bring none, a DOM whose one root is
    /// the model (feet at the origin, facing -Z); the noob when never set.
    pub fn set_character(&mut self, template: WeakDom) {
        self.shared.borrow_mut().players.template = Some(template);
    }

    /// Where avatars come from (`GetHumanoidDescriptionFromUserId`,
    /// `ApplyDescription`).
    pub fn set_avatars(&mut self, avatars: Box<dyn Avatars>) {
        self.shared.borrow_mut().players.avatars = Some(avatars);
    }

    /// A client's player and its character; on a server or solo run, the
    /// first player to join.
    pub fn local_player(&self) -> Option<(Ref, Option<Ref>)> {
        let shared = self.shared.borrow();
        let player = shared
            .players
            .local
            .or_else(|| shared.players.members.first().map(|m| m.player))?;
        let dom = self.dom.borrow();
        dom.get(player)?;
        Some((
            player,
            crate::humanoid::reference(&dom, player, "Character"),
        ))
    }

    /// Starts every script the server would run, in tree order, and runs
    /// their bodies up to their first yield, at time zero.
    pub fn start(&mut self) {
        let roots = self.dom.borrow().root_refs().to_vec();
        for root in roots {
            self.start_scripts_under(root);
        }
        self.rebuild();
        let changes = self.vm.resume();
        self.apply(changes);
        self.collect_output();
    }

    pub fn dom(&self) -> CellRef<'_, WeakDom> {
        self.dom.borrow()
    }

    /// Game time, seconds since the Run started.
    pub fn time(&self) -> f64 {
        self.frame as f64 / FRAME_HZ
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// The simulation's DOM, shared: an editor writes edits made during the
    /// run into it as logged changes, which the next [`Simulation::step`]
    /// routes.
    pub fn dom_rc(&self) -> Rc<RefCell<WeakDom>> {
        self.dom.clone()
    }

    /// Every change the simulation made to its DOM since the last call, in
    /// order: what scripts and edits logged, plus a `CFrame` write for each
    /// part physics moved (physics writes it unlogged). Empty until
    /// [`Simulation::watch_view_changes`].
    pub fn take_view_changes(&mut self) -> Vec<Change> {
        self.view_changes
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Starts keeping changes for [`Simulation::take_view_changes`]
    /// (headless runs don't, so nothing piles up).
    pub fn watch_view_changes(&mut self) {
        self.view_changes.get_or_insert_with(Vec::new);
    }

    /// What scripts printed since the last call.
    pub fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }

    /// Runs frames until `seconds` of game time have passed.
    pub fn run_for(&mut self, seconds: f64) {
        let frames = (seconds * FRAME_HZ).round() as u64;
        for _ in 0..frames {
            self.step();
        }
    }

    /// One frame, in the order a real server runs it (the RunServiceOrder
    /// golden): `PreAnimation`, `Stepped`, `PreSimulation`, physics,
    /// `PostSimulation`, the threads whose wait is over (`task.wait`,
    /// `task.delay`), `Heartbeat`, the step's `Touched`/`TouchEnded`
    /// (after `Heartbeat`: the TouchedOrder golden), the deferred queue.
    pub fn step(&mut self) {
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        let dt = 1.0 / FRAME_HZ;
        let started = self.time();
        self.frame += 1;
        let now = self.time();
        self.vm.set_time(now);

        self.fire(Event::RunService("PreAnimation", vec![dt]));
        self.animate(dt);
        self.fire(Event::RunService("Stepped", vec![started, dt]));
        self.fire(Event::RunService("PreSimulation", vec![dt]));

        let touches = self.physics_step();

        self.fire(Event::RunService("PostSimulation", vec![dt]));
        self.resume();
        self.fire(Event::RunService("Heartbeat", vec![dt]));
        self.fire_touches(touches);
        self.resume();
        self.collect_output();
        self.client_frame(dt);
    }

    /// `game:IsLoaded()`; turning it on fires `game.Loaded`.
    pub(crate) fn set_loaded(&mut self) {
        let changes = self.vm.set_loaded(true);
        self.apply(changes);
        self.collect_output();
    }

    /// Fires the instance events of DOM changes made from outside (by
    /// replication) and feeds them to physics.
    pub(crate) fn flush(&mut self) {
        let changes = self.vm.flush_changes();
        self.apply(changes);
    }

    pub(crate) fn fire_signal(&mut self, owner: Ref, name: &str, args: Vec<rbx_lua::server::Arg>) {
        self.fire(Event::Signal {
            owner,
            name: name.into(),
            args,
        });
    }

    pub(crate) fn destroy(&mut self, referent: Ref) {
        let changes = self.vm.destroy(referent);
        self.apply(changes);
    }

    pub(crate) fn dom_mut(&self) -> std::cell::RefMut<'_, WeakDom> {
        self.dom.borrow_mut()
    }

    pub(crate) fn db(&self) -> &'static ReflectionDatabase {
        self.db
    }

    /// The changes kept for replication since the last call.
    pub(crate) fn take_stream(&mut self) -> Vec<(u64, Change)> {
        self.stream.as_mut().map(std::mem::take).unwrap_or_default()
    }

    pub(crate) fn tombstone(&self, referent: Ref) -> Option<(String, String)> {
        self.vm.tombstone(referent)
    }

    pub(crate) fn take_outgoing(&mut self) -> Vec<rbx_lua::server::Outgoing> {
        self.vm.take_outgoing()
    }

    /// A remote packet from `from` (`None`: the server).
    pub(crate) fn receive(&mut self, from: Option<Ref>, packet: rbx_lua::server::Packet) {
        let changes = self.vm.receive(from, packet);
        self.apply(changes);
        self.collect_output();
    }

    pub(crate) fn ownership_mut(&self) -> std::cell::RefMut<'_, crate::ownership::Ownership> {
        std::cell::RefMut::map(self.shared.borrow_mut(), |s| &mut s.ownership)
    }

    /// Whether this peer moves `part` itself (the others are moved by
    /// replication).
    pub(crate) fn simulates(&self, part: Ref) -> bool {
        self.world.is_part(part) && !self.world.is_remote(part)
    }

    /// Where the unanchored parts this peer simulates are.
    pub(crate) fn moved_parts(&self) -> Vec<(Ref, rbx_dom::CFrameData)> {
        let dom = self.dom.borrow();
        self.world
            .moving_parts()
            .filter_map(|part| match dom.get(part)?.properties().get("CFrame") {
                Some(Variant::CFrame(cf)) => Some((part, *cf)),
                _ => None,
            })
            .collect()
    }

    /// Moves parts another peer simulates to where it put them, unlogged
    /// as physics moves are.
    pub(crate) fn place_parts(&mut self, parts: &[(Ref, rbx_dom::CFrameData)]) {
        for (part, cf) in parts {
            if self.simulates(*part) {
                continue;
            }
            {
                let mut dom = self.dom.borrow_mut();
                let Some(instance) = dom.get_mut(*part) else {
                    continue;
                };
                instance
                    .properties_mut()
                    .insert("CFrame".into(), Variant::CFrame(*cf));
            }
            self.world.teleport(*part, cf);
            if let Some(seen) = &mut self.view_changes {
                seen.push(Change::Property {
                    referent: *part,
                    name: "CFrame".into(),
                });
            }
        }
    }

    /// Every humanoid's state, and whether this peer decides it.
    pub(crate) fn humanoid_states(&self) -> Vec<(Ref, State, bool)> {
        let shared = self.shared.borrow();
        shared
            .humanoids
            .characters
            .values()
            .map(|c| (c.humanoid, c.state, !self.world.is_remote(c.root)))
            .collect()
    }

    /// A state another peer's humanoid went into.
    pub(crate) fn set_humanoid_state(&mut self, humanoid: Ref, state: State) {
        {
            let mut shared = self.shared.borrow_mut();
            let Some(c) = shared.humanoids.characters.get(&humanoid) else {
                return;
            };
            if c.state == state || !self.world.is_remote(c.root) {
                return;
            }
            shared.humanoids.replicate_state(humanoid, state);
        }
        self.humanoid_events();
    }

    pub(crate) fn take_track_ops(&mut self) -> Vec<(Option<Ref>, TrackOp)> {
        std::mem::take(&mut self.shared.borrow_mut().animations.ops)
    }

    /// Plays here what another peer did to one of its tracks.
    pub(crate) fn mirror_track(&mut self, from: Option<Ref>, op: TrackOp) {
        let events = {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            shared.animations.mirror(&mut dom, self.db, from, op);
            std::mem::take(&mut shared.animations.events)
        };
        for event in events {
            self.fire(event);
        }
    }

    pub(crate) fn humanoid_root(&self, humanoid: Ref) -> Option<Ref> {
        self.shared
            .borrow()
            .humanoids
            .characters
            .get(&humanoid)
            .map(|c| c.root)
    }

    pub(crate) fn take_forwarded(&mut self) -> Vec<(Ref, String, Vec<rbx_lua::server::Arg>)> {
        std::mem::take(&mut self.shared.borrow_mut().forwarded)
    }

    /// A humanoid call the server handed to this peer, its owner.
    pub(crate) fn humanoid_call(
        &mut self,
        humanoid: Ref,
        method: &str,
        args: &[rbx_lua::server::Arg],
    ) {
        let mut dom = self.dom.borrow_mut();
        let _ = self
            .shared
            .borrow_mut()
            .humanoids
            .call(&mut dom, self.db, humanoid, method, args);
    }

    /// The server's auto ownership for the frame, then each assembly's
    /// body made dynamic or kinematic as its owner says.
    fn sync_ownership(&mut self) {
        if self.role == Role::Server {
            let characters: Vec<(Ref, std::collections::HashSet<Ref>, [f64; 3])> = {
                let dom = self.dom.borrow();
                let shared = self.shared.borrow();
                shared
                    .players
                    .members
                    .iter()
                    .filter_map(|m| {
                        let model = crate::humanoid::reference(&dom, m.player, "Character")?;
                        let root = shared
                            .humanoids
                            .characters
                            .values()
                            .find(|c| c.model == model)
                            .map(|c| c.root)?;
                        let at = match dom.get(root)?.properties().get("CFrame") {
                            Some(Variant::CFrame(cf)) => {
                                [cf.position.x, cf.position.y, cf.position.z].map(f64::from)
                            }
                            _ => return None,
                        };
                        Some((
                            m.player,
                            scripts::subtree(&dom, model).into_iter().collect(),
                            at,
                        ))
                    })
                    .collect()
            };
            let assemblies = self.world.unanchored();
            let dom = self.dom.borrow();
            let mut shared = self.shared.borrow_mut();
            shared.ownership.retain(&dom);
            shared
                .ownership
                .assign(self.frame, &assemblies, &characters, |part| {
                    match dom.get(part)?.properties().get("CFrame") {
                        Some(Variant::CFrame(cf)) => {
                            Some([cf.position.x, cf.position.y, cf.position.z].map(f64::from))
                        }
                        _ => None,
                    }
                });
        }
        self.world.sync_ownership();
    }

    fn resume(&mut self) {
        let changes = self.vm.resume();
        self.apply(changes);
    }

    fn physics_step(&mut self) -> Vec<Touch> {
        let (gravity, fallen_height) = self.workspace_numbers();
        self.players_step();
        {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            shared.humanoids.frame = self.frame;
            shared.humanoids.sync(&mut dom, self.db);
        }
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        if self.dirty {
            self.rebuild();
        }
        let mut drives: Vec<Drive> = {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            shared
                .humanoids
                .before_physics(&mut dom, self.db, &self.world, gravity)
        };
        self.humanoid_events();
        // Deaths and seats can break or make joints: rebuild now.
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        if self.dirty {
            self.rebuild();
        }
        self.sync_ownership();
        let steps = (PHYSICS_HZ / FRAME_HZ).round() as u32;
        self.world.refresh_constraints(&self.dom.borrow(), self.db);
        let touches = self
            .world
            .step(gravity, steps, self.adaptive_stepping(), &mut drives);
        self.world.write_back(&mut self.dom.borrow_mut(), self.db);
        self.world.write_constraints(&mut self.dom.borrow_mut());
        crate::animation::lay_out(&mut self.dom.borrow_mut(), &self.shared.borrow().animations);
        self.shared
            .borrow_mut()
            .humanoids
            .after_physics(&self.world, &drives);
        self.humanoid_events();
        if let Some(seen) = &mut self.view_changes {
            let dom = self.dom.borrow();
            seen.extend(
                self.world
                    .moving_parts()
                    .filter(|part| dom.get(*part).is_some())
                    .map(|referent| Change::Property {
                        referent,
                        name: "CFrame".to_string(),
                    }),
            );
        }
        let fallen = self.world.fallen(fallen_height, self.role != Role::Client);
        for part in fallen {
            // A client destroys only what it made; the server the rest.
            if self.role == Role::Client && !crate::ownership::client_made(part) {
                continue;
            }
            let changes = self.vm.destroy(part);
            self.apply(changes);
        }
        touches
    }

    /// Joining, characters loading and respawning, appearance events, old
    /// characters destroyed.
    fn players_step(&mut self) {
        let due = {
            let dom = self.dom.borrow();
            let mut shared = self.shared.borrow_mut();
            shared.players.frame = self.frame;
            let Shared {
                players, humanoids, ..
            } = &mut *shared;
            players.due(&dom, self.db, humanoids)
        };
        for due in due {
            match due {
                Due::Fire(event) => self.fire(event),
                Due::Load(player) => self.load_character(player),
            }
        }
        let old = std::mem::take(&mut self.shared.borrow_mut().players.old);
        for character in old {
            let changes = self.vm.destroy(character);
            self.apply(changes);
        }
    }

    /// `LoadCharacter` from the engine (auto-load, respawn): the same three
    /// steps a script's call takes, scripts running between them.
    fn load_character(&mut self, player: Ref) {
        let removing = self
            .shared
            .borrow()
            .players
            .removing(&self.dom.borrow(), player);
        if let Some((owner, name, args)) = removing {
            self.fire(Event::Signal { owner, name, args });
        }
        let added = {
            let mut dom = self.dom.borrow_mut();
            self.shared
                .borrow_mut()
                .players
                .build(&mut dom, self.db, player)
        };
        if let Some((owner, name, args)) = added {
            self.fire(Event::Signal { owner, name, args });
        }
        {
            let mut dom = self.dom.borrow_mut();
            self.shared
                .borrow_mut()
                .players
                .place(&mut dom, self.db, player);
        }
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
    }

    /// Steps every Animator `dt` and fires what its tracks did.
    fn animate(&mut self, dt: f64) {
        let events = {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            shared.animations.step(&mut dom, dt);
            std::mem::take(&mut shared.animations.events)
        };
        for event in events {
            self.fire(event);
        }
    }

    /// Where animations not in the asset cache are downloaded from.
    pub fn set_fetcher(&mut self, fetcher: std::sync::Arc<dyn rbx_assets::AssetFetcher>) {
        self.shared.borrow_mut().animations.fetcher = Some(fetcher);
    }

    /// Fires what the humanoids queued and destroys the joints they broke.
    fn humanoid_events(&mut self) {
        let (events, destroy) = {
            let mut shared = self.shared.borrow_mut();
            (
                std::mem::take(&mut shared.humanoids.events),
                std::mem::take(&mut shared.humanoids.destroy),
            )
        };
        for event in events {
            self.fire(event);
        }
        for joint in destroy {
            let changes = self.vm.destroy(joint);
            self.apply(changes);
        }
    }

    /// `Touched`/`TouchEnded` for both parts of each touch.
    fn fire_touches(&mut self, touches: Vec<Touch>) {
        for touch in &touches {
            if touch.began {
                let mut dom = self.dom.borrow_mut();
                self.shared
                    .borrow_mut()
                    .humanoids
                    .touched(&mut dom, self.db, touch.a, touch.b);
            }
        }
        self.humanoid_events();
        for touch in touches {
            for (part, other) in [(touch.a, touch.b), (touch.b, touch.a)] {
                self.fire(if touch.began {
                    Event::Touched { part, other }
                } else {
                    Event::TouchEnded { part, other }
                });
            }
        }
    }

    fn fire(&mut self, event: Event) {
        let changes = self.vm.fire(event);
        self.apply(changes);
    }

    fn rebuild(&mut self) {
        let groups = self.vm.collision_groups();
        self.world
            .rebuild(&self.dom.borrow(), self.db, &groups.borrow());
        self.dirty = false;
    }

    /// `Workspace.PhysicsSteppingMethod`: only `Fixed` keeps everything
    /// at 240 Hz; `Default` stepped adaptively on the server we recorded.
    fn adaptive_stepping(&self) -> bool {
        const FIXED: u32 = 1;
        let dom = self.dom.borrow();
        let method = dom
            .root_refs()
            .iter()
            .filter_map(|r| dom.get(*r))
            .find(|i| i.class() == "Workspace")
            .and_then(|w| self.db.stored_or_default(w, "PhysicsSteppingMethod"))
            .map(|(_, v)| v.clone());
        !matches!(method, Some(Variant::Enum(FIXED)))
    }

    fn workspace_numbers(&self) -> (f64, f64) {
        let dom = self.dom.borrow();
        let workspace = dom
            .root_refs()
            .iter()
            .filter_map(|r| dom.get(*r))
            .find(|i| i.class() == "Workspace");
        let number = |name: &str, default: f64| {
            workspace
                .and_then(|w| self.db.stored_or_default(w, name))
                .and_then(|(_, v)| match v {
                    Variant::Float32(x) => Some(f64::from(*x)),
                    Variant::Float64(x) => Some(*x),
                    _ => None,
                })
                .unwrap_or(default)
        };
        (
            number("Gravity", DEFAULT_GRAVITY),
            number("FallenPartsDestroyHeight", DEFAULT_FALLEN_HEIGHT),
        )
    }

    fn start_scripts_under(&mut self, root: Ref) {
        let found: Vec<Ref> = {
            let dom = self.dom.borrow();
            let client = self.shared.borrow().players.client_roots(&dom);
            let role = self.role;
            scripts::subtree(&dom, root)
                .into_iter()
                .filter(|r| {
                    (role != Role::Client && scripts::runs(&dom, *r))
                        || (role != Role::Server && scripts::runs_on_client(&dom, *r, &client))
                })
                .collect()
        };
        for script in found {
            if let Err(err) = self.vm.start_script(script) {
                self.output
                    .push_text(rbx_lua::LineKind::Error, err.to_string());
            }
        }
    }

    /// Feeds what scripts changed to physics: a moved part moves its body,
    /// a velocity write sets it, anything structural rebuilds the world
    /// before the next step. Scripts newly placed where they run start.
    fn apply(&mut self, changes: Vec<Change>) {
        if let Some(seen) = &mut self.view_changes {
            seen.extend(changes.iter().cloned());
        }
        if let Some(stream) = &mut self.stream {
            let first = self.dom.borrow().mutations() + 1 - changes.len() as u64;
            stream.extend(
                changes
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, c)| (first + i as u64, c)),
            );
        }

        for change in changes {
            match change {
                Change::Property { referent, name } => self.property_changed(referent, &name),
                Change::Added(referent) | Change::Parent { referent, .. } => {
                    if self.touches_physics(referent) {
                        self.dirty = true;
                    }
                    self.start_scripts_under(referent);
                }
                Change::Removed(referent) => {
                    if self.world.is_part(referent) || self.world.is_joint(referent) {
                        self.dirty = true;
                    }
                }
                Change::Class(_) => self.dirty = true,
            }
        }
    }

    fn property_changed(&mut self, referent: Ref, name: &str) {
        let dom = self.dom.borrow();
        let Some(instance) = dom.get(referent) else {
            return;
        };
        let class = instance.class();
        let is_part = self.db.is_subclass_of(class, "BasePart");
        let is_joint = self.db.is_subclass_of(class, "JointInstance") || class == "WeldConstraint";
        // Constraint parameters are re-read every frame; what a constraint
        // joins (and where its attachments sit) needs a rebuild.
        if crate::physics::is_constraint(class) {
            if matches!(
                name,
                "Attachment0" | "Attachment1" | "Enabled" | "LimitsEnabled" | "Mode"
            ) || name.ends_with("AlignmentMode")
            {
                self.dirty = true;
            }
            return;
        }
        if self.db.is_subclass_of(class, "Attachment") {
            if self.world.is_joint(referent) && name != "Name" {
                self.dirty = true;
            }
            return;
        }
        if !(is_part || is_joint) {
            if matches!(class, "Script" | "LocalScript") && name != "Source" {
                drop(dom);
                self.start_scripts_under(referent);
            }
            return;
        }
        if STRUCTURAL.contains(&name) || (is_part && !self.world.is_part(referent)) {
            self.dirty = true;
            return;
        }
        if !is_part {
            return;
        }
        let read = |n: &str| {
            self.db
                .stored_or_default(instance, n)
                .map(|(_, v)| v.clone())
        };
        match name {
            "CFrame" => {
                if let Some(Variant::CFrame(cf)) = read("CFrame") {
                    drop(dom);
                    self.world.teleport(referent, &cf);
                }
            }
            "AssemblyLinearVelocity" | "Velocity" | "AssemblyAngularVelocity" | "RotVelocity" => {
                let angular = matches!(name, "AssemblyAngularVelocity" | "RotVelocity");
                if let Some(Variant::Vector3(v)) = instance.properties().get(name).cloned() {
                    drop(dom);
                    self.world.set_velocity(referent, v, angular);
                }
            }
            _ => {}
        }
    }

    fn touches_physics(&self, root: Ref) -> bool {
        let dom = self.dom.borrow();
        scripts::subtree(&dom, root).into_iter().any(|r| {
            dom.get(r).is_some_and(|i| {
                let class = i.class();
                self.db.is_subclass_of(class, "BasePart")
                    || self.db.is_subclass_of(class, "JointInstance")
                    || class == "WeldConstraint"
                    || crate::physics::is_constraint(class)
            })
        })
    }

    fn collect_output(&mut self) {
        self.output.extend(self.vm.take_output());
    }
}
