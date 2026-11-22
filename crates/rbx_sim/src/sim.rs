//! The frame loop: scripts and physics stepped together.

use std::cell::{Ref as CellRef, RefCell};
use std::rc::Rc;

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Event, Physics, ServerVm};
use rbx_lua::Output;
use rbx_reflection::ReflectionDatabase;

use crate::animation::TrackOp;
use crate::humanoid::State;
use crate::input::{Input, Inputs, MouseBehavior};
use crate::physics::{Bridge, Drive, Shared, Touch, World, PHYSICS_HZ};

/// A part as the peer simulating it left it this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub part: Ref,
    pub cframe: rbx_dom::CFrameData,
    pub linear: rbx_dom::Vector3Data,
    pub angular: rbx_dom::Vector3Data,
}
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
    "Attachment0",
    "Attachment1",
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
    /// The collision groups the world was last built with: a script
    /// changing which groups collide rebuilds it.
    built_groups: rbx_lua::server::CollisionGroups,
    /// Roots added under Workspace, and parts and joints taken out of it,
    /// since the last build: unless `dirty`, the world takes just those.
    added: Vec<Ref>,
    removed: Vec<Ref>,
    /// Built parts whose shape or surface changed: their assemblies are
    /// built again.
    refit: Vec<Ref>,
    output: Output,
    /// Changes kept for [`Simulation::take_view_changes`], once watched.
    view_changes: Option<Vec<Change>>,
    /// The same, kept for a recording's frame hashes.
    traced: Option<Vec<Change>>,
    /// While a replicated tree arrives: the roots whose scripts start once
    /// all of it is here (see [`Simulation::hold_scripts`]).
    held_scripts: Option<Vec<Ref>>,
    /// Play (local): the local client's input, `None` for a server-only run.
    inputs: Option<Inputs>,
    role: Role,
    /// Logged changes with their DOM mutation number, for a session to
    /// replicate; `None` outside one.
    stream: Option<Vec<(u64, Change)>>,
    /// Parts another peer simulates that a script here moved.
    overruled: Vec<Ref>,
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
            built_groups: Default::default(),
            added: Vec::new(),
            removed: Vec::new(),
            refit: Vec::new(),
            output: Output::default(),
            view_changes: None,
            traced: None,
            held_scripts: None,
            inputs: None,
            role: Role::Server,
            stream: None,
            overruled: Vec::new(),
        })
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// See [`rbx_lua::server::ServerVm::set_script_timeout`].
    pub fn set_script_timeout(&mut self, timeout: std::time::Duration) {
        self.vm.set_script_timeout(timeout);
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
        let now = self.time();
        let sent = self.vm.take_virtual_input();
        inputs.pending.extend(crate::input::from_virtual(sent));
        for input in inputs.take_pending() {
            if let Input::Gamepad { pad, connected } = input {
                let changes = self.vm.gamepad(uis, pad, connected);
                self.apply(changes);
                continue;
            }
            let written = inputs.write(&mut self.dom.borrow_mut(), self.db, &input);
            let Some(written) = written else {
                continue;
            };
            let object = written.object;
            let handled = {
                let dom = self.dom.borrow();
                self.shared
                    .borrow_mut()
                    .gui
                    .input(&dom, self.db, &input, object, uis)
            };
            self.gui_steps(handled.before);
            let changes =
                self.vm
                    .input_with(object, uis, written.signal, written.also, handled.processed);
            self.apply(changes);
            self.gui_steps(handled.after);
            if let Input::Wheel { notches } = input {
                use rbx_lua::server::Arg;
                let args = vec![
                    Arg::Number(f64::from(notches)),
                    Arg::Vector2([0.0, 0.0]),
                    Arg::Number(0.0),
                    Arg::Bool(handled.processed),
                ];
                self.fire_signal(uis, "PointerAction", args);
            }
            if let Input::Touch {
                id,
                position,
                phase,
            } = input
            {
                let target = self.shared.borrow().gui.touch_target(position);
                let at = [position[0], position[1] - crate::gui::TOPBAR_INSET];
                let gestures = inputs
                    .touches
                    .touch(id, phase, at, now, target, handled.processed);
                self.fire_gestures(uis, gestures);
            }
            if handled.processed {
                continue;
            }
            self.tool_input(&input);
            if matches!(&input, Input::Key { code, down: true } if code == "Space") {
                let changes = self.vm.fire(Event::Signal {
                    owner: uis,
                    name: "JumpRequest".into(),
                    args: Vec::new(),
                });
                self.apply(changes);
            }
        }
        let gestures = inputs.touches.tick(now);
        self.fire_gestures(uis, gestures);
        self.inputs = Some(inputs);
        // The hotbar keeps each tool's slot from when it first came.
        let player = self.shared.borrow().players.local;
        if let Some(player) = player {
            let dom = self.dom.borrow();
            self.shared.borrow_mut().tools.hotbar(&dom, player);
        }
        let changes = self.vm.render_step(dt);
        self.apply(changes);
        self.collect_output();
    }

    /// Lays out the local player's `PlayerGui` (its objects'
    /// `AbsolutePosition` and `AbsoluteSize`, the camera's `ViewportSize`)
    /// and fires what changed.
    fn gui_layout(&mut self) {
        let player = self.shared.borrow().players.local;
        {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            let viewport = shared.gui.viewport;
            crate::input::set_viewport(&mut dom, viewport);
            let root = player.and_then(|p| {
                dom.get(p)?
                    .children()
                    .iter()
                    .copied()
                    .find(|c| dom.get(*c).is_some_and(|i| i.class() == "PlayerGui"))
            });
            if let Some(root) = root {
                shared.gui.relayout(&mut dom, self.db, root);
            }
        }
        self.flush();
    }

    /// A key or click the game left: the hotbar's number keys, Backspace,
    /// a tool's activation.
    fn tool_input(&mut self, input: &Input) {
        let Some(player) = self.shared.borrow().players.local else {
            return;
        };
        let fire = {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            let shown = shared.gui.core_enabled("Backpack");
            shared.tools.input(&mut dom, self.db, player, input, shown)
        };
        self.flush();
        for (owner, name, args) in fire {
            self.fire_signal(owner, &name, args);
        }
    }

    /// Parts another peer simulates that a script here moved since the
    /// last call.
    pub(crate) fn take_overruled(&mut self) -> Vec<Ref> {
        std::mem::take(&mut self.overruled)
    }

    /// Players a script kicked since the last call, with the message.
    pub(crate) fn take_kicked(&mut self) -> Vec<(Ref, String)> {
        std::mem::take(&mut self.shared.borrow_mut().players.kicked)
    }

    /// A player leaves the server: `PlayerRemoving`, then their character
    /// and the player go.
    pub(crate) fn remove_player(&mut self, player: Ref) {
        let players = self.dom.borrow().parent(player);
        if let Some(players) = players {
            self.fire_signal(
                players,
                "PlayerRemoving",
                vec![rbx_lua::server::Arg::Instance(player)],
            );
        }
        let character = crate::humanoid::reference(&self.dom.borrow(), player, "Character");
        if let Some(character) = character {
            self.destroy(character);
        }
        let user_id = match self
            .dom
            .borrow()
            .get(player)
            .and_then(|i| i.properties().get("UserId"))
        {
            Some(Variant::Int64(id)) => Some(*id),
            _ => None,
        };
        let sources =
            user_id.map_or_else(Vec::new, |id| crate::chat::sources(&self.dom.borrow(), id));
        for source in sources {
            self.destroy(source);
        }
        if self.dom.borrow().get(player).is_some() {
            self.destroy(player);
        }
        self.shared
            .borrow_mut()
            .players
            .members
            .retain(|m| m.player != player);
    }

    /// A client's tool moves and activations since the last call, for the
    /// server.
    pub(crate) fn take_tool_ops(&mut self) -> Vec<crate::tools::ClientOp> {
        std::mem::take(&mut self.shared.borrow_mut().tools.ops)
    }

    /// Marks a client applying the server's frame (its tool moves are the
    /// server's, not the player's).
    pub(crate) fn set_remote(&mut self, remote: bool) {
        self.shared.borrow_mut().tools.remote = remote;
    }

    /// What `player`'s client did with its tools, on the server.
    pub(crate) fn apply_tool_ops(&mut self, player: Ref, ops: Vec<crate::tools::ClientOp>) {
        for op in ops {
            match op {
                crate::tools::ClientOp::Move(tool, parent) => {
                    let allowed = {
                        let dom = self.dom.borrow();
                        crate::tools::allowed(&dom, self.db, player, tool, parent)
                    };
                    if allowed {
                        self.dom.borrow_mut().set_parent(tool, parent);
                        self.flush();
                    }
                }
                crate::tools::ClientOp::Activate(tool, on) => {
                    let name = if on { "Activated" } else { "Deactivated" };
                    self.fire_signal(tool, name, Vec::new());
                }
                crate::tools::ClientOp::Reset(humanoid) => {
                    let mine = {
                        let dom = self.dom.borrow();
                        let character = crate::humanoid::reference(&dom, player, "Character");
                        character.is_some() && dom.parent(humanoid) == character
                    };
                    if mine {
                        crate::humanoid::set(
                            &mut self.dom.borrow_mut(),
                            self.db,
                            humanoid,
                            "Health",
                            Variant::Float32(0.0),
                        );
                        self.flush();
                    }
                }
                crate::tools::ClientOp::MoveToFinished(humanoid, reached) => {
                    let owned = self
                        .dom
                        .borrow()
                        .get(humanoid)
                        .is_some_and(|i| i.class() == "Humanoid");
                    if owned {
                        self.fire_signal(
                            humanoid,
                            "MoveToFinished",
                            vec![rbx_lua::server::Arg::Bool(reached)],
                        );
                    }
                }
                crate::tools::ClientOp::Sound(sound, playing) => {
                    let replicates = {
                        let dom = self.dom.borrow();
                        dom.get(sound).is_some_and(|i| i.class() == "Sound")
                            && crate::sound::replicates(&dom, self.db)
                    };
                    if replicates {
                        self.shared.borrow_mut().sounds.client_played(
                            &mut self.dom.borrow_mut(),
                            sound,
                            playing,
                        );
                        self.flush();
                    }
                }
            }
        }
    }

    /// What the default core GUI shows now (see [`crate::gui::CoreGui`]);
    /// the notifications are handed over once.
    pub fn core_gui(&mut self) -> crate::gui::CoreGui {
        let local = self.shared.borrow().players.local;
        let mut core = crate::gui::CoreGui {
            notifications: std::mem::take(&mut self.shared.borrow_mut().gui.notifications),
            ..Default::default()
        };
        let Some(player) = local else {
            return core;
        };
        let (health, backpack, list) = {
            let shared = self.shared.borrow();
            let gui = &shared.gui;
            core.topbar = !gui.core_flag("TopbarEnabled", false);
            core.chat =
                core.topbar && gui.core_enabled("Chat") && !gui.core_flag("ChatActive", false);
            core.chat_bar = core.chat && !gui.core_flag("ChatBarDisabled", true);
            (
                core.topbar && gui.core_enabled("Health"),
                core.topbar && gui.core_enabled("Backpack"),
                core.topbar && gui.core_enabled("PlayerList"),
            )
        };
        if backpack {
            core.hotbar = Some(self.hotbar());
        }
        let dom = self.dom.borrow();
        if health {
            let humanoid = crate::humanoid::reference(&dom, player, "Character").and_then(|c| {
                dom.get(c)?
                    .children()
                    .iter()
                    .copied()
                    .find(|h| dom.get(*h).is_some_and(|i| i.class() == "Humanoid"))
            });
            core.health = humanoid.map(|h| {
                (
                    crate::humanoid::number(&dom, self.db, h, "Health"),
                    crate::humanoid::number(&dom, self.db, h, "MaxHealth"),
                )
            });
        }
        if let Some((camera, _)) = crate::input::camera(&dom) {
            use rbx_lua::server::Physics as _;
            let bridge = Bridge(self.shared.clone());
            let eye = [camera.position.x, camera.position.y, camera.position.z];
            core.labels =
                crate::gui::name_labels(&dom, self.db, player, eye, &|from, to, parts| {
                    let filter = rbx_lua::server::QueryFilter {
                        parts,
                        include: false,
                        respect_can_collide: false,
                        collision_group: "Default".into(),
                        max_parts: 0,
                        ignore_water: true,
                    };
                    let direction = [0, 1, 2].map(|i| f64::from(to[i] - from[i]));
                    bridge
                        .raycast(from.map(f64::from), direction, &filter)
                        .is_some()
                });
        }
        core.can_reset = !matches!(
            self.shared.borrow().gui.reset_callback(),
            Some(rbx_lua::server::Arg::Bool(false))
        );
        for p in crate::players::players_of(&dom) {
            let head = crate::humanoid::reference(&dom, p, "Character").and_then(|c| {
                let head = dom
                    .get(c)?
                    .children()
                    .iter()
                    .copied()
                    .find(|h| dom.get(*h).is_some_and(|i| i.name() == "Head"))?;
                match dom.get(head)?.properties().get("CFrame") {
                    Some(Variant::CFrame(cf)) => {
                        Some([cf.position.x, cf.position.y, cf.position.z])
                    }
                    _ => None,
                }
            });
            let user_id = match dom.get(p).and_then(|i| i.properties().get("UserId")) {
                Some(Variant::Int64(id)) => *id,
                _ => 0,
            };
            if let Some(head) = head {
                core.heads.push((user_id, head));
            }
        }
        if list {
            let stats = |p: Ref| -> Vec<(String, String)> {
                let Some(folder) = dom.get(p).and_then(|i| {
                    i.children()
                        .iter()
                        .copied()
                        .find(|c| dom.get(*c).is_some_and(|f| f.name() == "leaderstats"))
                }) else {
                    return Vec::new();
                };
                // Added order, then a child `Priority` NumberValue (higher
                // first), an `IsPrimary` BoolValue before all (leaderboard
                // docs; the M18 probe saw added order, not names').
                let mut ranked: Vec<(bool, f64, String, String)> = dom
                    .get(folder)
                    .map(|f| f.children().to_vec())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|c| {
                        let i = dom.get(c)?;
                        // A value never written (or written back to its
                        // default) still shows its default: an IntValue's 0.
                        let value = self.db.stored_or_default(i, "Value").map(|(_, v)| v);
                        let text = stat_text(value)?;
                        let marker = |name: &str| {
                            i.children()
                                .iter()
                                .filter_map(|m| dom.get(*m))
                                .find(|m| m.name() == name)
                                .and_then(|m| m.properties().get("Value").cloned())
                        };
                        let primary = matches!(marker("IsPrimary"), Some(Variant::Bool(true)));
                        let priority = match marker("Priority") {
                            Some(Variant::Float64(v)) => v,
                            Some(Variant::Float32(v)) => f64::from(v),
                            _ => 0.0,
                        };
                        Some((primary, priority, i.name().to_owned(), text))
                    })
                    .collect();
                ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
                // The real list shows four stats at most (a six-stat probe).
                ranked.truncate(4);
                ranked
                    .into_iter()
                    .map(|(_, _, name, text)| (name, text))
                    .collect()
            };
            let all = crate::players::players_of(&dom);
            for p in &all {
                for (name, _) in stats(*p) {
                    if !core.stats.contains(&name) {
                        core.stats.push(name);
                    }
                }
            }
            let players =
                all.into_iter()
                    .filter_map(|p| {
                        let i = dom.get(p)?;
                        let mine = stats(p);
                        let values = core
                            .stats
                            .iter()
                            .map(|column| {
                                mine.iter()
                                    .find(|(n, _)| n == column)
                                    .map(|(_, v)| v.clone())
                                    .unwrap_or_default()
                            })
                            .collect();
                        let team = crate::humanoid::reference(&dom, p, "Team").and_then(|t| {
                            match dom.get(t)?.properties().get("TeamColor") {
                                Some(Variant::BrickColor(c)) => {
                                    rbx_dom::BrickColor::from_number(*c).map(|b| b.rgb)
                                }
                                _ => None,
                            }
                        });
                        let display_name = match i.properties().get("DisplayName") {
                            Some(Variant::String(d)) if !d.is_empty() => d.clone(),
                            _ => i.name().to_owned(),
                        };
                        Some(crate::gui::PlayerRow {
                            user_id: match i.properties().get("UserId") {
                                Some(Variant::Int64(id)) => *id,
                                _ => 0,
                            },
                            name: i.name().to_owned(),
                            display_name,
                            team,
                            stats: values,
                        })
                    })
                    .collect();
            core.players = Some(players);
        }
        core
    }

    /// Whether the Escape menu is up: `GuiService.MenuIsOpen`, which our
    /// camera reads to let the mouse go and stop turning while the player
    /// is busy with the menu, as Roblox's does.
    pub fn set_menu_open(&mut self, open: bool) {
        let mut dom = self.dom.borrow_mut();
        let gui = crate::input::service(&mut dom, "GuiService");
        let Some(instance) = dom.get(gui) else {
            return;
        };
        if instance.properties().get("MenuIsOpen") == Some(&rbx_dom::Variant::Bool(open)) {
            return;
        }
        let _ = dom.set_property(gui, "MenuIsOpen", rbx_dom::Variant::Bool(open));
        drop(dom);
        self.fire(Event::Signal {
            owner: gui,
            name: if open { "MenuOpened" } else { "MenuClosed" }.into(),
            args: Vec::new(),
        });
    }

    /// The menu's Reset button: what `ResetButtonCallback` asks for, a
    /// `BindableEvent` fired, nothing when it's `false`, or by default the
    /// character's death (on the server, where health is kept).
    pub fn reset_character(&mut self) {
        let callback = self.shared.borrow().gui.reset_callback().cloned();
        match callback {
            Some(rbx_lua::server::Arg::Bool(false)) => {}
            Some(rbx_lua::server::Arg::Instance(event)) => {
                self.fire_signal(event, "Event", Vec::new());
            }
            _ => {
                let Some((_, Some(character))) = self.local_player() else {
                    return;
                };
                let humanoid = {
                    let dom = self.dom.borrow();
                    dom.get(character).and_then(|c| {
                        c.children()
                            .iter()
                            .copied()
                            .find(|h| dom.get(*h).is_some_and(|i| i.class() == "Humanoid"))
                    })
                };
                if let Some(humanoid) = humanoid {
                    self.shared
                        .borrow_mut()
                        .tools
                        .ops
                        .push(crate::tools::ClientOp::Reset(humanoid));
                }
            }
        }
    }

    /// The local player's hotbar: each tool's name, icon and whether it's
    /// held.
    pub fn hotbar(&self) -> Vec<crate::gui::HotbarSlot> {
        let Some(player) = self.shared.borrow().players.local else {
            return Vec::new();
        };
        let dom = self.dom.borrow();
        let tools = self.shared.borrow_mut().tools.hotbar(&dom, player);
        let character = crate::humanoid::reference(&dom, player, "Character");
        tools
            .into_iter()
            .filter_map(|t| {
                let i = dom.get(t)?;
                let icon = match i.properties().get("TextureId") {
                    Some(Variant::Content(rbx_dom::Content::Uri(id)) | Variant::String(id)) => {
                        id.clone()
                    }
                    _ => String::new(),
                };
                Some(crate::gui::HotbarSlot {
                    name: i.name().to_owned(),
                    icon,
                    held: character.is_some() && dom.parent(t) == character,
                })
            })
            .collect()
    }

    /// Runs what the GUI does for an input: its signals, and its text
    /// edits with their change events.
    fn gui_steps(&mut self, steps: Vec<crate::gui::Step>) {
        for step in steps {
            match step {
                crate::gui::Step::Fire(event) => self.fire(event),
                crate::gui::Step::Text(r, text) => {
                    let _ = self
                        .dom
                        .borrow_mut()
                        .set_property(r, "Text", Variant::String(text));
                    self.flush();
                }
            }
        }
    }

    /// The window the client draws in, pixels (`Camera.ViewportSize`, what
    /// `ScreenGui`s lay out in).
    pub fn set_viewport(&mut self, size: [f32; 2]) {
        self.shared.borrow_mut().gui.viewport = size;
    }

    /// The device's safe-area insets (left, top, right, bottom).
    pub fn set_safe_insets(&mut self, insets: [f32; 4]) {
        self.shared.borrow_mut().gui.safe_insets = insets;
    }

    /// The client's GUI state: focus, the core GUI switches, notifications.
    pub fn gui(&self) -> std::cell::Ref<'_, crate::gui::Gui> {
        std::cell::Ref::map(self.shared.borrow(), |s| &s.gui)
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
        if let Some(player) = player {
            let mut dom = self.dom.borrow_mut();
            self.shared
                .borrow_mut()
                .teams
                .assign(&mut dom, self.db, player);
            crate::chat::add_source(&mut dom, name, user_id);
        }
        // `Players.ChildAdded` (and `DescendantAdded` up the tree) fire for
        // the new player, as for any child.
        let edits = self.vm.flush_changes();
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
    pub fn set_avatars(&mut self, avatars: std::rc::Rc<dyn Avatars>) {
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
        if self.role != Role::Client {
            crate::chat::setup(&mut self.dom.borrow_mut(), self.db);
            crate::input::server_camera(&mut self.dom.borrow_mut());
        }
        let roots = self.dom.borrow().root_refs().to_vec();
        for root in roots {
            self.watch_sounds_under(root);
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

    /// Keeps `changes` for the view and the replay trace, whichever watch.
    fn seen(&mut self, changes: &[Change]) {
        for kept in [&mut self.view_changes, &mut self.traced]
            .into_iter()
            .flatten()
        {
            kept.extend_from_slice(changes);
        }
    }

    /// The changes since the last call, as [`Simulation::take_view_changes`]
    /// has them, for a recording's frame hashes.
    pub(crate) fn take_trace(&mut self) -> Vec<Change> {
        self.traced.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Record or replay: the clock, randomness and the web's answers go
    /// through `tape`, background asset loads land when `gate` says, and
    /// the changes made are kept for the frame hashes.
    pub(crate) fn set_tape(&mut self, tape: rbx_lua::tape::Tape, gate: crate::replay::Gate) {
        if let Err(err) = self.vm.set_tape(tape.clone()) {
            self.output
                .push_text(rbx_lua::LineKind::Error, err.to_string());
        }
        let mut shared = self.shared.borrow_mut();
        shared.players.tape = Some(tape);
        shared.sounds.gate = Some(gate.clone());
        shared.animations.gate = Some(gate.clone());
        shared.videos.gate = Some(gate);
        drop(shared);
        self.traced.get_or_insert_with(Vec::new);
    }

    /// What scripts printed since the last call.
    pub fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }

    /// The server shuts down as Roblox's does: its `BindToClose` functions
    /// run, frame after frame, until they have all returned or 30 seconds
    /// of game time have passed.
    pub fn close(&mut self) {
        let changes = self.vm.close();
        self.apply(changes);
        let deadline = self.frame + (30.0 * FRAME_HZ) as u64;
        while !self.vm.closed() && self.frame < deadline {
            self.step();
        }
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
        // A client lays its GUI out once what replication brought is in,
        // before any script of the frame reads it.
        if self.inputs.is_some() {
            self.gui_layout();
        }
        let dt = 1.0 / FRAME_HZ;
        let started = self.time();
        self.frame += 1;
        let now = self.time();
        self.vm.set_time(now);
        // `Workspace.DistributedGameTime`: the server's game time, which
        // its clients get with the rest of the frame.
        if self.role != Role::Client {
            let mut dom = self.dom.borrow_mut();
            let workspace = dom
                .root_refs()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"));
            if let Some(workspace) = workspace {
                let _ = dom.set_property(workspace, "DistributedGameTime", Variant::Float64(now));
            }
        }

        // Tweens step first (TweenRuntime `phase`: a tween played after
        // Heartbeat has moved by PreAnimation, on a client by
        // PreSimulation after its render step).
        let changes = self.vm.tween_step(dt);
        self.apply(changes);
        self.sound_step(dt);
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

    /// Touch gestures, on `uis` or their GuiObject.
    fn fire_gestures(&mut self, uis: Ref, gestures: Vec<crate::touch::Gesture>) {
        for (owner, name, args) in gestures {
            self.fire_signal(owner.unwrap_or(uis), name, args);
        }
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

    /// The round trip to the server, seconds: what `Player:GetNetworkPing()`
    /// returns and, on a client, `Stats.Network.ServerStatsItem["Data
    /// Ping"]` reads (in milliseconds).
    pub(crate) fn set_network_ping(&mut self, seconds: f64) {
        self.vm.set_network_ping(seconds);
        let mut dom = self.dom_mut();
        let child = |dom: &WeakDom, parent: Ref, name: &str| {
            dom.get(parent)?
                .children()
                .iter()
                .copied()
                .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
        };
        let stats = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Stats"));
        let item = stats
            .and_then(|s| child(&dom, s, "Network"))
            .and_then(|n| child(&dom, n, "ServerStatsItem"))
            .and_then(|s| child(&dom, s, "Data Ping"));
        if let Some(instance) = item.and_then(|i| dom.get_mut(i)) {
            instance
                .properties_mut()
                .insert("Value".into(), Variant::Float64(seconds * 1000.0));
        }
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
    /// [`Self::moved_parts`], plus the parts that moved last time and came
    /// to rest since (`last`), at rest: the others' copies stop too.
    pub(crate) fn moved_parts_since(
        &self,
        last: &mut std::collections::HashSet<Ref>,
    ) -> Vec<Placed> {
        let mut moved = self.moved_parts();
        let now: std::collections::HashSet<Ref> = moved.iter().map(|p| p.part).collect();
        let stopped: Vec<Ref> = last.difference(&now).copied().collect();
        moved.extend(self.poses(&stopped));
        *last = now;
        moved
    }

    /// `parts`' poses here, at rest.
    pub(crate) fn poses(&self, parts: &[Ref]) -> Vec<Placed> {
        let dom = self.dom.borrow();
        let zero = rbx_dom::Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        parts
            .iter()
            .filter_map(|&part| match dom.get(part)?.properties().get("CFrame") {
                Some(Variant::CFrame(cframe)) => Some(Placed {
                    part,
                    cframe: *cframe,
                    linear: zero,
                    angular: zero,
                }),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn moved_parts(&self) -> Vec<Placed> {
        let dom = self.dom.borrow();
        self.world
            .moving_parts()
            .filter_map(|part| {
                let cframe = match dom.get(part)?.properties().get("CFrame") {
                    Some(Variant::CFrame(cf)) => *cf,
                    _ => return None,
                };
                let v = |name| {
                    let [x, y, z] =
                        crate::humanoid::vector(&dom, self.db, part, name).map(|c| c as f32);
                    rbx_dom::Vector3Data { x, y, z }
                };
                Some(Placed {
                    part,
                    cframe,
                    linear: v("AssemblyLinearVelocity"),
                    angular: v("AssemblyAngularVelocity"),
                })
            })
            .collect()
    }

    /// Moves parts another peer simulates to where it put them, unlogged
    /// as physics moves are.
    pub(crate) fn place_parts(&mut self, parts: &[Placed]) {
        for placed in parts {
            let (part, cf) = (&placed.part, &placed.cframe);
            if self.simulates(*part) {
                continue;
            }
            {
                let mut dom = self.dom.borrow_mut();
                let Some(instance) = dom.get_mut(*part) else {
                    continue;
                };
                // Anchored here since the peer moved it (a server script
                // anchoring a character it then teleports): this side's
                // pose stands.
                if matches!(
                    instance.properties().get("Anchored"),
                    Some(Variant::Bool(true))
                ) {
                    continue;
                }
                let class = instance.class().to_owned();
                let props = instance.properties_mut();
                props.insert("CFrame".into(), Variant::CFrame(*cf));
                // The owner's velocity comes along: the part reads it, and
                // it carries the part on between two updates.
                for (name, value) in [
                    ("AssemblyLinearVelocity", placed.linear),
                    ("AssemblyAngularVelocity", placed.angular),
                ] {
                    let key = self
                        .db
                        .stored_names(&class, name)
                        .first()
                        .copied()
                        .unwrap_or(name);
                    props.insert(key.into(), Variant::Vector3(value));
                }
            }
            self.world.teleport(*part, cf);
            self.shared.borrow_mut().moved.insert(*part);
            self.seen(&[Change::Property {
                referent: *part,
                name: "CFrame".into(),
            }]);
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
            let number = |player: Ref, name: &str| match dom
                .get(player)
                .and_then(|i| i.properties().get(name))
            {
                Some(Variant::Float32(v)) => Some(f64::from(*v)),
                Some(Variant::Float64(v)) => Some(*v),
                _ => None,
            };
            let reach = |player: Ref| {
                let max =
                    number(player, "MaximumSimulationRadius").unwrap_or(crate::ownership::REACH);
                number(player, "SimulationRadius").unwrap_or(max).min(max)
            };
            shared.ownership.assign(
                self.frame,
                &assemblies,
                &characters,
                |part| match dom.get(part)?.properties().get("CFrame") {
                    Some(Variant::CFrame(cf)) => {
                        Some([cf.position.x, cf.position.y, cf.position.z].map(f64::from))
                    }
                    _ => None,
                },
                reach,
            );
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
        self.explode();
        {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            shared.humanoids.frame = self.frame;
            shared.humanoids.sync(&mut dom, self.db);
        }
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        self.rebuild_if_changed();
        let mut drives: Vec<Drive> = {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            let shared = &mut *shared;
            shared.humanoids.before_physics(
                &mut dom,
                self.db,
                &self.world,
                &shared.terrain,
                gravity,
            )
        };
        self.humanoid_events();
        // Deaths and seats can break or make joints: rebuild now.
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
        self.rebuild_if_changed();
        self.sync_ownership();
        let steps = (PHYSICS_HZ / FRAME_HZ).round() as u32;
        self.world.refresh_constraints(&self.dom.borrow(), self.db);
        if self.role == Role::Client {
            self.world.set_listened(self.vm.touch_listeners());
        }
        let touches = self
            .world
            .step(gravity, steps, self.adaptive_stepping(), &mut drives);
        self.world.write_back(&mut self.dom.borrow_mut(), self.db);
        self.world.write_constraints(&mut self.dom.borrow_mut());
        let posed =
            crate::animation::lay_out(&mut self.dom.borrow_mut(), &self.shared.borrow().animations);
        self.shared.borrow_mut().moved.extend(posed);
        self.shared
            .borrow_mut()
            .humanoids
            .after_physics(&self.world, &drives);
        self.humanoid_events();
        if self.view_changes.is_some() || self.traced.is_some() {
            let moved: Vec<Change> = {
                let dom = self.dom.borrow();
                self.world
                    .moving_parts()
                    .filter(|part| dom.get(*part).is_some())
                    .map(|referent| Change::Property {
                        referent,
                        name: "CFrame".to_string(),
                    })
                    .collect()
            };
            self.seen(&moved);
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
        // Placing fires its own events (the character's Workspace.ChildAdded,
        // the PlayerGui's copies arriving), so a script waiting on them goes on.
        let edits = self.vm.flush_changes();
        self.apply(edits);
    }

    /// `ParticleEmitter:Emit`/`Clear` calls since the last call.
    pub fn take_bursts(&mut self) -> Vec<(Ref, crate::Burst)> {
        std::mem::take(&mut self.shared.borrow_mut().bursts)
    }

    /// The `ParticleEmitter2D:Emit(count)`/`Clear()` calls scripts made
    /// since the last call, for the GUI renderer.
    pub fn take_gui_bursts(&mut self) -> Vec<(Ref, crate::Burst)> {
        std::mem::take(&mut self.shared.borrow_mut().gui_bursts)
    }

    /// The chat messages this client showed since the last call.
    pub fn take_chat(&mut self) -> Vec<rbx_lua::server::ChatLine> {
        let mut lines = self.vm.take_chat();
        let system = std::mem::take(&mut self.shared.borrow_mut().gui.system_messages);
        lines.extend(system.into_iter().map(|text| rbx_lua::server::ChatLine {
            prefix: String::new(),
            text,
            channel: "RBXSystem".into(),
            user_id: 0,
        }));
        lines
    }

    /// The local player says `text` in the general channel (the editor's
    /// chat bar).
    pub fn chat_say(&mut self, text: &str) {
        let changes = self.vm.chat_say(text);
        self.apply(changes);
        self.collect_output();
    }

    /// Moves every playing sound's and audio player's clock `dt` and fires
    /// what that did (and what the audio wiring did); a sound that ended
    /// stops once its `Ended` handlers have run. A client renders its audio
    /// graph here too.
    fn sound_step(&mut self, dt: f64) {
        let (mut events, ended) = {
            let dom = self.dom.borrow();
            self.shared.borrow_mut().sounds.step(&dom, self.db, dt)
        };
        {
            let mut dom = self.dom.borrow_mut();
            let mut shared = self.shared.borrow_mut();
            let Shared {
                audio,
                sounds,
                videos,
                players,
                ..
            } = &mut *shared;
            events.extend(audio.step(sounds, &mut dom, self.db, dt, players.client, players.local));
            events.extend(videos.step(&mut dom, self.db, dt));
        }
        for event in events {
            self.fire(event);
        }
        {
            let mut dom = self.dom.borrow_mut();
            for sound in ended {
                crate::sound::set(&mut dom, sound, false, false);
            }
        }
        let edits = self.dom.borrow_mut().take_changes();
        self.apply(edits);
    }

    /// The sounds playing now as the camera hears them, for the editor's
    /// audio output.
    pub fn voices(&self) -> Vec<crate::audio::Voice> {
        let camera = self.camera().map(|(cf, _)| cf);
        let dom = self.dom.borrow();
        let listener = crate::sound::listener(&dom, self.db, camera);
        self.shared
            .borrow_mut()
            .sounds
            .voices(&dom, self.db, listener.as_ref())
    }

    /// What this client's audio graph played since the last call, stereo
    /// frames at [`crate::audio_graph::RATE`].
    pub fn take_audio(&self) -> Vec<crate::audio_graph::Frame> {
        std::mem::take(&mut self.shared.borrow_mut().audio.out)
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

    /// The DataStores, purchases and web lookups this peer's scripts use;
    /// models load through the fetcher sounds use, unless they say.
    pub fn set_services(&mut self, mut config: rbx_lua::server::Services) {
        if config.assets.is_none() {
            config.assets = self
                .shared
                .borrow()
                .sounds
                .fetcher
                .clone()
                .map(|f| std::sync::Arc::new(Models(f)) as _);
        }
        if let Some(inputs) = &mut self.inputs {
            inputs.emulate_touch = config.device.touch;
            self.shared.borrow_mut().gui.touch = config.device.touch;
            // A console comes with its gamepad.
            if config.device.gamepad {
                inputs.pending.push(Input::Gamepad {
                    pad: 1,
                    connected: true,
                });
            }
        }
        self.vm.set_services(config);
    }

    /// The purchase prompts scripts asked for since the last call.
    pub fn take_prompts(&self) -> Vec<rbx_lua::server::PurchasePrompt> {
        self.vm.take_prompts()
    }

    /// The teleports into reloadable places scripts asked for since the
    /// last call.
    pub fn take_teleports(&self) -> Vec<rbx_lua::server::TeleportRequest> {
        self.vm.take_teleports()
    }

    /// The `RequestStreamAroundAsync` calls scripts made since the last
    /// call.
    pub(crate) fn take_stream_requests(&self) -> Vec<rbx_lua::server::StreamRequest> {
        self.vm.take_stream_requests()
    }

    pub(crate) fn stream_request_done(&self, id: u64) {
        self.vm.stream_request_done(id)
    }

    pub(crate) fn replication_foci(&self, player: Ref) -> Vec<Ref> {
        self.vm.replication_foci(player)
    }

    pub(crate) fn persistent_players(&self, model: Ref) -> Vec<Ref> {
        self.vm.persistent_players(model)
    }

    /// The toasts scripts asked the core GUI for since the last call.
    pub fn take_notices(&self) -> Vec<rbx_lua::server::Notice> {
        self.vm.take_notices()
    }

    /// The test purchase dialog for `prompt` closed.
    pub fn purchase_finished(&mut self, prompt: &rbx_lua::server::PurchasePrompt, bought: bool) {
        let changes = self.vm.purchase_finished(prompt, bought);
        self.apply(changes);
        self.collect_output();
    }

    /// Where animations not in the asset cache are downloaded from.
    pub fn set_fetcher(&mut self, fetcher: std::sync::Arc<dyn rbx_assets::AssetFetcher>) {
        let mut shared = self.shared.borrow_mut();
        shared.animations.fetcher = Some(fetcher.clone());
        shared.videos.fetcher = Some(fetcher.clone());
        shared.sounds.fetcher = Some(fetcher);
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
            if self.role == Role::Client {
                if let rbx_lua::server::Event::Signal { owner, name, args } = &event {
                    if name == "MoveToFinished" {
                        let reached =
                            matches!(args.first(), Some(rbx_lua::server::Arg::Bool(true)));
                        self.shared
                            .borrow_mut()
                            .tools
                            .ops
                            .push(crate::tools::ClientOp::MoveToFinished(*owner, reached));
                    }
                }
            }
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

    /// Takes `server`'s collision groups: the server registers them and
    /// they replicate, so a client's `SetPartCollisionGroup`, `CollisionGroup`
    /// writes and queries know the server's groups.
    pub fn sync_collision_groups(&mut self, server: &Simulation) {
        let theirs = server.vm.collision_groups();
        let mine = self.vm.collision_groups();
        if *mine.borrow() != *theirs.borrow() {
            *mine.borrow_mut() = theirs.borrow().clone();
            self.dirty = true;
        }
    }

    fn rebuild_if_changed(&mut self) {
        let groups = self.vm.collision_groups();
        if *groups.borrow() != self.built_groups {
            self.built_groups = groups.borrow().clone();
            self.dirty = true;
        }
        if self.dirty
            || !self.added.is_empty()
            || !self.removed.is_empty()
            || !self.refit.is_empty()
        {
            self.rebuild();
        }
    }

    /// Builds the world again, or only what was added and removed when
    /// that is enough.
    fn rebuild(&mut self) {
        let added = std::mem::take(&mut self.added);
        let removed = std::mem::take(&mut self.removed);
        let refit = std::mem::take(&mut self.refit);
        let groups = self.vm.collision_groups();
        let groups = groups.borrow();
        let dom = self.dom.borrow();
        if !self.dirty {
            let present = |r| dom.get(r).is_some() && in_workspace(&dom, r);
            let roots: Vec<Ref> = added.into_iter().filter(|&r| present(r)).collect();
            if self.world.remove(&removed, present)
                && self.world.refit(&dom, self.db, &groups, &refit)
                && self.world.add(&dom, self.db, &groups, &roots)
            {
                return;
            }
        }
        self.world.rebuild(&dom, self.db, &groups);
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

    /// Scripts placed from now on wait for [`Simulation::release_scripts`]:
    /// a client's LocalScript in a replicated ScreenGui starts with all its
    /// siblings there, as in Roblox, though the instances (and their
    /// events) arrive one at a time.
    pub(crate) fn hold_scripts(&mut self) {
        self.held_scripts.get_or_insert_with(Vec::new);
    }

    pub(crate) fn holding_scripts(&self) -> bool {
        self.held_scripts.is_some()
    }

    pub(crate) fn release_scripts(&mut self) {
        for root in self.held_scripts.take().unwrap_or_default() {
            self.start_scripts_under(root);
        }
    }

    fn start_scripts_under(&mut self, root: Ref) {
        if let Some(held) = &mut self.held_scripts {
            held.push(root);
            return;
        }
        let found: Vec<Ref> = {
            let dom = self.dom.borrow();
            let client = self.shared.borrow().players.client_roots(&dom);
            let role = self.role;
            let mut under = scripts::subtree(&dom, root);
            // The local player can arrive after its character (a client
            // joining a place whose character already spawned): the
            // character's LocalScripts (Animate) start with the player.
            if let [player, character] = client[..] {
                if under.contains(&player) {
                    under.extend(scripts::subtree(&dom, character));
                }
            }
            under
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
        self.seen(&changes);
        let first = self.dom.borrow().mutations() + 1 - changes.len() as u64;
        self.shared
            .borrow_mut()
            .note(&self.dom.borrow(), first, &changes);
        if let Some(stream) = &mut self.stream {
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
                // A character can arrive before the player's `Character`
                // names it (a respawn): its LocalScripts start then.
                Change::Property { referent, name } if name == "Character" => {
                    let character = match self
                        .dom
                        .borrow()
                        .get(referent)
                        .map(|i| i.properties().get("Character").cloned())
                    {
                        Some(Some(Variant::Ref(r))) => Some(r),
                        _ => None,
                    };
                    if let Some(character) = character {
                        self.start_scripts_under(character);
                    }
                }
                Change::Property { referent, name } => self.property_changed(referent, &name),
                Change::Added(referent) | Change::Parent { referent, .. } => {
                    if !in_workspace(&self.dom.borrow(), referent) {
                        let dom = self.dom.borrow();
                        self.removed.extend(
                            scripts::subtree(&dom, referent)
                                .into_iter()
                                .filter(|r| self.world.is_part(*r) || self.world.is_joint(*r)),
                        );
                    } else if self.touches_physics(referent) {
                        self.added.push(referent);
                    }
                    // A client's Terrain arrives with the place, voxels and
                    // all.
                    let terrain = crate::terrain::find(&self.dom.borrow());
                    if terrain.is_some() && self.shared.borrow().terrain.referent != terrain {
                        let dom = self.dom.borrow();
                        self.shared.borrow_mut().terrain.load(&dom);
                    }
                    self.watch_sounds_under(referent);
                    if in_workspace(&self.dom.borrow(), referent) {
                        let dom = self.dom.borrow();
                        let under = scripts::subtree(&dom, referent);
                        self.shared.borrow_mut().explosions.watch(&dom, &under);
                    }
                    self.start_scripts_under(referent);
                }
                Change::Removed(referent) => {
                    if self.world.is_part(referent) || self.world.is_joint(referent) {
                        self.removed.push(referent);
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
        // Terrain's voxels come back from an edit or the server, not a shape.
        if class == "Terrain" {
            if matches!(name, "SmoothGrid" | "MaterialColors") {
                self.shared.borrow_mut().terrain.changed(&dom, referent);
            }
            return;
        }
        let is_part = self.db.is_subclass_of(class, "BasePart");
        let is_joint = self.db.is_subclass_of(class, "JointInstance")
            || matches!(class, "WeldConstraint" | "RigidConstraint");
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
        // A part outside Workspace (a template in ReplicatedStorage being
        // posed) has no body: moving it rebuilds nothing.
        let placed = in_workspace(&dom, referent);
        let unbuilt = is_part && !self.world.is_part(referent) && placed;
        if unbuilt {
            self.added.push(referent);
            return;
        }
        if STRUCTURAL.contains(&name) && placed {
            // A part's own shape and surface: its assembly is built again.
            if is_part {
                self.refit.push(referent);
            } else {
                self.dirty = true;
            }
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
                    if self.world.is_remote(referent) {
                        self.overruled.push(referent);
                    }
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

    /// Sets off the explosions put in Workspace since the last step: their
    /// `Hit`s, broken joints and pushes (`explosion.rs`).
    fn explode(&mut self) {
        let lifetime = (crate::explosion::LIFETIME * crate::FRAME_HZ) as u64;
        let expired = self
            .shared
            .borrow_mut()
            .explosions
            .expired(self.frame, lifetime);
        for explosion in expired {
            if self.dom.borrow().get(explosion).is_some() {
                self.destroy(explosion);
            }
        }
        let pending = self.shared.borrow_mut().explosions.take(self.frame);
        for explosion in pending {
            let blast = {
                let dom = self.dom.borrow();
                if !in_workspace(&dom, explosion) {
                    continue;
                }
                let bridge = crate::physics::Bridge(self.shared.clone());
                crate::explosion::blast(&bridge, &dom, self.db, explosion)
            };
            for joint in blast.joints {
                self.destroy(joint);
            }
            {
                let bridge = crate::physics::Bridge(self.shared.clone());
                for (part, impulse) in &blast.impulses {
                    bridge.apply_impulse(*part, *impulse, false);
                }
            }
            for (part, distance) in blast.hits {
                self.fire(Event::Signal {
                    owner: explosion,
                    name: "Hit".into(),
                    args: vec![
                        rbx_lua::server::Arg::Instance(part),
                        rbx_lua::server::Arg::Number(distance),
                    ],
                });
            }
        }
    }

    fn watch_sounds_under(&mut self, root: Ref) {
        let dom = self.dom.borrow();
        let mut shared = self.shared.borrow_mut();
        for r in scripts::subtree(&dom, root) {
            match dom.get(r).map(|i| i.class()) {
                Some("Sound") => shared.sounds.watch(r),
                Some(class) if crate::video::watches(class) => shared.videos.watch(r),
                Some(class) if crate::audio_api::watches(class) => shared.audio.watch(r),
                _ => {}
            }
        }
    }

    /// Whether `root`'s arrival or move changes the world: physical
    /// instances that came into Workspace, or ones with bodies that left.
    /// Whether the subtree at `root` (in Workspace) has anything physics
    /// builds.
    fn touches_physics(&self, root: Ref) -> bool {
        let dom = self.dom.borrow();
        scripts::subtree(&dom, root).into_iter().any(|r| {
            dom.get(r).is_some_and(|i| {
                let class = i.class();
                self.db.is_subclass_of(class, "BasePart")
                    || self.db.is_subclass_of(class, "JointInstance")
                    || matches!(class, "WeldConstraint" | "RigidConstraint")
                    || crate::physics::is_constraint(class)
            })
        })
    }

    fn collect_output(&mut self) {
        self.output.extend(self.vm.take_output());
        let failures = {
            let mut shared = self.shared.borrow_mut();
            let mut all = std::mem::take(&mut shared.sounds.failures);
            all.append(&mut shared.animations.failures);
            all
        };
        for line in failures {
            self.output.push_text(rbx_lua::LineKind::Warn, line);
        }
    }
}

/// Whether `r` is under Workspace, where parts have bodies.
fn in_workspace(dom: &WeakDom, r: Ref) -> bool {
    let mut at = Some(r);
    while let Some(current) = at {
        let Some(instance) = dom.get(current) else {
            return false;
        };
        if instance.class() == "Workspace" {
            return true;
        }
        at = dom.parent(current);
    }
    false
}

/// A `leaderstats` value as the player list shows it (M18 probe on a real
/// client): thousands separated by commas, a fraction kept; once that runs
/// past 7 characters, cut to K, M or B with one decimal under 100 (12.3K,
/// 1.2M, 123M, 1B, 1,000B). A BoolValue isn't shown.
fn stat_text(value: Option<&Variant>) -> Option<String> {
    Some(match value? {
        Variant::Int64(v) => stat_number(*v as f64),
        Variant::Int32(v) => stat_number(f64::from(*v)),
        Variant::Float64(v) => stat_number(*v),
        Variant::Float32(v) => stat_number(f64::from(*v)),
        Variant::String(v) => v.clone(),
        _ => return None,
    })
}

fn stat_number(v: f64) -> String {
    let commas = |whole: u64| {
        let digits = whole.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let sign = if v < 0.0 { "-" } else { "" };
    let a = v.abs();
    // Rust's shortest form gives the fraction as typed (1234.1, not
    // 1234.0999…).
    let typed = format!("{a}");
    let fraction = typed.find('.').map_or("", |dot| &typed[dot..]);
    let plain = format!("{sign}{}{fraction}", commas(a.trunc() as u64));
    if plain.trim_start_matches('-').len() <= 7 || !a.is_finite() {
        return plain;
    }
    let (scaled, suffix) = match a {
        a if a >= 1e9 => (a / 1e9, "B"),
        a if a >= 1e6 => (a / 1e6, "M"),
        _ => (a / 1e3, "K"),
    };
    let body = if scaled < 100.0 {
        let tenths = (scaled * 10.0).trunc() / 10.0;
        let text = format!("{tenths:.1}");
        text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
    } else {
        commas(scaled.trunc() as u64)
    };
    format!("{sign}{body}{suffix}")
}

/// `AssetService:LoadAssetAsync`'s models and `ContentProvider`'s
/// preloads: the asset cache first (an offline pack, an earlier run), else
/// the asset fetcher, kept.
struct Models(std::sync::Arc<dyn rbx_assets::AssetFetcher>);

impl std::fmt::Debug for Models {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Models")
    }
}

impl rbx_lua::server::AssetSource for Models {
    fn fetch(&self, id: u64) -> Result<Vec<u8>, String> {
        let cache = rbx_assets::AssetCache::new(None).ok();
        if let Some(bytes) = crate::replay::cached(Some(&self.0), id) {
            return Ok(bytes);
        }
        let bytes = self.0.fetch_id(id).map_err(|e| match e {
            rbx_assets::FetchError::NotFound(_) => "HTTP 404 (Not Found)".to_owned(),
            other => other.to_string(),
        })?;
        if let Some(cache) = cache {
            let _ = cache.put_id(id, &bytes);
        }
        Ok(bytes)
    }

    /// Content shipped with the client (`rbxasset://`) and thumbnails are
    /// always there to a client; an asset id has to be had.
    fn preload(&self, content: &str) -> Result<(), String> {
        match rbx_assets::AssetRef::parse(content) {
            Ok(rbx_assets::AssetRef::Id(id)) => self.fetch(id).map(drop),
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

#[cfg(test)]
mod stat_tests {
    use super::stat_number;

    #[test]
    fn leaderstats_numbers_read_as_the_real_client_shows_them() {
        for (value, shown) in [
            (0.0, "0"),
            (0.5, "0.5"),
            (1000.0, "1,000"),
            (1000.5, "1,000.5"),
            (9999.5, "9,999.5"),
            (12345.0, "12,345"),
            (12345.5, "12.3K"),
            (123456.5, "123K"),
            (766001.0, "766,001"),
            (766001.5, "766K"),
            (1234567.0, "1.2M"),
            (12345678.0, "12.3M"),
            (123456789.0, "123M"),
            (1e9, "1B"),
            (1e12, "1,000B"),
            (1e15, "1,000,000B"),
        ] {
            assert_eq!(stat_number(value), shown, "{value}");
        }
    }
}
