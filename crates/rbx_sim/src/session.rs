//! A local game: one server and a client per local player, each its own
//! VM and DataModel in this process, with our own replication between
//! them (not Roblox's protocol; the observable rules are from the
//! ReplicationOrder, RemoteOrder, RemoteArgs and Ownership goldens):
//!
//! - the server's changes reach a client at the end of the server's frame,
//!   in the order they were made, interleaved with its remote events;
//!   several writes to one property in a frame arrive once, where the
//!   first was, with the value at the end of the frame;
//! - a new instance arrives with its properties, then its children one at
//!   a time, each firing its events before the next comes;
//! - `ServerStorage` and `ServerScriptService` never replicate, nor does
//!   another player's `PlayerGui`; an instance moved there leaves the
//!   client as a parent change to nil and comes back as the same object;
//! - nothing a client changes replicates, except the physics of what it
//!   owns, its humanoids' states and its animation tracks; remotes go
//!   both ways, unreliable ones after the frame's reliable traffic;
//! - a client starts with `ReplicatedFirst` alone, its LocalScripts
//!   running before the rest of the place arrives and `game.Loaded` fires;
//! - with `Workspace.StreamingEnabled`, the parts and atomic models in
//!   Workspace stream in and out around the player's focus
//!   (`crate::streaming`).

use std::collections::HashSet;

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, Outgoing, Packet, To};

use crate::animation::TrackOp;
use crate::humanoid::State;
use crate::sim::{Placed, Simulation};
use crate::streaming::{self, Integrity, Kind, Settings};
use crate::{Input, Output};

/// Frames between two looks at what a streaming client should have: what
/// comes into range lands ~0.1 s later (the streaming goldens).
const STREAM_EVERY: u64 = 6;
/// Frames from the join's first look (the area around the focus, right
/// after `game.Loaded`) to the rest of the target radius (~0.2 s).
const STREAM_FIRST: u64 = 12;
/// Frames an old focus position or a requested area stays streamed in.
const STREAM_KEEP: u64 = 303;

/// Services whose contents a client never gets.
const SERVER_ONLY: &[&str] = &["ServerStorage", "ServerScriptService"];

/// The origin a client gives the server's own track operations.
const SERVER: Ref = Ref::new(u32::MAX);

/// Who printed a line of output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Server,
    /// Local player number n (0-based).
    Client(usize),
}

pub struct Client {
    pub sim: Simulation,
    pub player: Ref,
    pub name: String,
    /// Everything but `ReplicatedFirst` arrives on the client's first frame.
    loaded: bool,
    /// `CharacterAdded` waiting for its character to arrive: (player, model).
    characters: Vec<(Ref, Ref)>,
    /// Its own character, held out of the join's snapshot for a frame: on
    /// Roblox it lands after ReplicatedFirst has started, so a loader there
    /// waiting on `CharacterAdded` sees it fire.
    deferred: Option<Ref>,
    /// What the client sends the server at the start of the next frame.
    outbox: Upstream,
    /// Parts it moved last frame, to say when they stop.
    moving: HashSet<Ref>,
    /// Parts the server simulates that it moved itself: the server's pose
    /// puts them back next frame, asleep or not (Ownership
    /// `client_view_server_owned`).
    overruled: Vec<Ref>,
    /// With `StreamingEnabled`: what of Workspace it has.
    stream: Option<Stream>,
}

/// A client's side of instance streaming.
struct Stream {
    settings: Settings,
    /// The units (`streaming::unit_of`) it has.
    have: HashSet<Ref>,
    /// `RequestStreamAroundAsync` calls for the next look.
    requests: Vec<rbx_lua::server::StreamRequest>,
    /// The frame of the next full look at what to stream.
    due: u64,
    /// Past the join's first look.
    started: bool,
    /// Where the focus was lately and the areas asked for, each kept like
    /// the minimum radius until its frame (Opportunistic streams those
    /// out 5 s after the focus left, the rest beyond the target radius at
    /// once).
    recent: Vec<([f32; 3], u64)>,
    paused: bool,
    persistent_loaded: bool,
}

#[derive(Default)]
struct Upstream {
    packets: Vec<Outgoing>,
    parts: Vec<Placed>,
    states: Vec<(Ref, State)>,
    tracks: Vec<TrackOp>,
    tools: Vec<crate::tools::ClientOp>,
}

/// What the server sends its clients after a frame.
enum Op {
    Property(Ref, String),
    /// Added or moved, with the parent it was given.
    Tree(Ref, Option<Ref>),
    Removed(Ref),
    Packet(Outgoing),
}

pub struct Session {
    pub server: Simulation,
    pub clients: Vec<Client>,
    /// Players kicked, with the message their client shows: (name,
    /// message), oldest first.
    pub kicked: Vec<(String, String)>,
    /// What a client printed before it left, not yet taken.
    left: Vec<(Peer, Output)>,
    /// Server parts moved this frame on a client's behalf, for the others.
    relayed: Vec<Placed>,
    /// Parts the server moved last frame, to say when they stop.
    moving: HashSet<Ref>,
    fetcher: Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>,
    avatars: Option<std::rc::Rc<dyn crate::players::Avatars>>,
    services: Option<rbx_lua::server::Services>,
    /// Teleport GUIs players bring into this run, by user id, placed in
    /// their client's DataModel as they join.
    arriving: std::collections::HashMap<i64, WeakDom>,
    script_timeout: Option<std::time::Duration>,
}

/// A teleport a peer's script asked for, with the teleport GUI copied out
/// of that peer's DataModel (a lone root).
pub struct Teleport {
    pub request: rbx_lua::server::TeleportRequest,
    pub gui: Option<WeakDom>,
}

/// Copies `root`'s subtree from `from` under `parent` in `into`, with fresh
/// referents (properties pointing at instances are left out); the copy's
/// root.
fn copy_subtree(from: &WeakDom, root: Ref, into: &mut WeakDom, parent: Option<Ref>) -> Option<Ref> {
    let instance = from.get(root)?;
    let made = into.new_instance(instance.class(), instance.name(), parent);
    for (name, value) in instance.properties() {
        if !matches!(value, rbx_dom::Variant::Ref(_)) {
            let _ = into.set_property(made, name, value.clone());
        }
    }
    for child in instance.children().to_vec() {
        copy_subtree(from, child, into, Some(made));
    }
    Some(made)
}

impl Session {
    /// A session over `dom`, the place as the server loads it.
    pub fn new(dom: WeakDom) -> Result<Session, String> {
        let mut server = Simulation::new(dom)?;
        server.make_server();
        Ok(Session {
            server,
            clients: Vec::new(),
            kicked: Vec::new(),
            left: Vec::new(),
            relayed: Vec::new(),
            moving: HashSet::new(),
            fetcher: None,
            avatars: None,
            services: None,
            arriving: std::collections::HashMap::new(),
            script_timeout: None,
        })
    }

    /// Starts the server's scripts. Players join after.
    pub fn start(&mut self) {
        self.server.start();
    }

    /// A local player joins as `name`, wearing `template` or the default
    /// character: the server adds their `Player`, and their client starts
    /// with `ReplicatedFirst` alone, its LocalScripts run; the rest comes
    /// on the session's next frame.
    pub fn join(
        &mut self,
        name: &str,
        user_id: i64,
        template: Option<WeakDom>,
    ) -> Result<usize, String> {
        let player = self
            .server
            .join(name, user_id, template)
            .ok_or("the place has no Players service")?;
        let index = self.clients.len();
        let settings = Settings::read(&self.server.dom());
        let mut view = first_view(&self.server.dom(), player);
        // With an integrity mode the client starts paused, until its
        // first look.
        if settings.is_some_and(|s| s.integrity != Integrity::Disabled) {
            let _ = view.set_property(player, "GameplayPaused", Variant::Bool(true));
            view.take_changes();
        }
        let mut sim = Simulation::new(view)?;
        sim.make_client(index, player);
        if let Some(timeout) = self.script_timeout {
            sim.set_script_timeout(timeout);
        }
        if let Some(fetcher) = &self.fetcher {
            sim.set_fetcher(fetcher.clone());
        }
        if let Some(avatars) = &self.avatars {
            sim.set_avatars(avatars.clone());
        }
        let mut services = self.services.clone();
        if let Some(gui) = self.arriving.remove(&user_id) {
            let root = gui.root_refs().first().copied();
            let made = root.and_then(|root| {
                let mut dom = sim.dom_mut();
                let made = copy_subtree(&gui, root, &mut dom, None)?;
                dom.unparent(made);
                dom.take_changes();
                Some(made)
            });
            if let Some(arrival) = services.as_mut().and_then(|s| s.arrivals.get_mut(&user_id)) {
                arrival.gui = made;
            }
        }
        if let Some(services) = services {
            sim.set_services(services);
        }
        sim.start();
        let stream = settings.map(|settings| Stream {
            settings,
            have: HashSet::new(),
            requests: Vec::new(),
            due: 0,
            started: false,
            recent: Vec::new(),
            paused: false,
            persistent_loaded: false,
        });
        self.clients.push(Client {
            sim,
            player,
            name: name.into(),
            loaded: false,
            characters: Vec::new(),
            deferred: None,
            outbox: Upstream::default(),
            moving: HashSet::new(),
            overruled: Vec::new(),
            stream,
        });
        Ok(index)
    }

    /// One frame: what the clients sent arrives at the server, the server
    /// steps, then each client gets the server's frame and steps.
    pub fn step(&mut self) {
        self.relayed.clear();
        for client in &mut self.clients {
            let up = std::mem::take(&mut client.outbox);
            let player = client.player;
            let (mut reliable, unreliable): (Vec<_>, Vec<_>) =
                up.packets.into_iter().partition(|o| {
                    !matches!(
                        o.packet,
                        Packet::Event {
                            unreliable: true,
                            ..
                        }
                    )
                });
            reliable.extend(unreliable);
            for out in reliable {
                if matches!(out.to, To::Server) {
                    self.server.receive(Some(player), out.packet);
                }
            }
            let owned = self.server.ownership_mut().owned_by(player);
            let parts: Vec<_> = up
                .parts
                .into_iter()
                .filter(|placed| owned.contains(&placed.part))
                .collect();
            self.server.place_parts(&parts);
            self.relayed.extend(parts);
            for (humanoid, state) in up.states {
                self.server.set_humanoid_state(humanoid, state);
            }
            for op in up.tracks {
                self.server.mirror_track(Some(player), op);
            }
            self.server.apply_tool_ops(player, up.tools);
        }

        self.server.step();

        let stream = self.server.take_stream();
        let packets = self.server.take_outgoing();
        let ops = frame_ops(stream, packets);
        let mut moved = self.server.moved_parts_since(&mut self.moving);
        moved.extend(self.relayed.iter().copied());
        let states: Vec<(Ref, State)> = self
            .server
            .humanoid_states()
            .into_iter()
            .map(|(h, s, _)| (h, s))
            .collect();
        let tracks = self.server.take_track_ops();
        let forwarded = self.server.take_forwarded();
        for request in self.server.take_stream_requests() {
            match self.clients.iter_mut().find(|c| c.player == request.player) {
                Some(Client {
                    stream: Some(stream),
                    ..
                }) => stream.requests.push(request),
                _ => self.server.stream_request_done(request.id),
            }
        }

        let server = &self.server;
        for client in &mut self.clients {
            let owned = server.ownership_mut().owned_by(client.player);
            // Paused, the client stops simulating its character.
            let paused = client.stream.as_ref().is_some_and(|s| s.paused);
            client.sim.ownership_mut().owned = if paused { HashSet::new() } else { owned };
            client.sim.sync_collision_groups(server);
            if !client.loaded {
                client.arrive(server);
                client.stream(server);
            } else {
                if let Some(character) = client.deferred.take() {
                    let parent = server.dom().parent(character);
                    if parent.is_some() {
                        client.deliver(server, &Op::Tree(character, parent));
                    }
                }
                for op in &ops {
                    client.deliver(server, op);
                }
                client.stream(server);
            }
            client.sim.place_parts(&moved);
            let overruled = server.poses(&std::mem::take(&mut client.overruled));
            client.sim.place_parts(&overruled);
            for (humanoid, state) in &states {
                client.sim.set_humanoid_state(*humanoid, *state);
            }
            for (origin, op) in &tracks {
                if *origin != Some(client.player) {
                    client.sim.mirror_track(Some(SERVER), op.clone());
                }
            }
            for (humanoid, method, args) in &forwarded {
                let root = client.sim.humanoid_root(*humanoid);
                if root.is_some_and(|r| client.sim.simulates(r)) {
                    client.sim.humanoid_call(*humanoid, method, args);
                }
            }
            client.sim.step();
            client.collect();
        }

        let mut kicked = self.server.take_kicked();
        for client in &mut self.clients {
            kicked.extend(client.sim.take_kicked());
        }
        for (player, message) in kicked {
            if let Some(index) = self.clients.iter().position(|c| c.player == player) {
                let name = self.clients[index].name.clone();
                self.leave(index);
                self.kicked.push((name, message));
            }
        }
    }

    /// Local player `index` leaves: the server's `PlayerRemoving`, their
    /// character and player go (the other clients see the same next
    /// frame), and their client stops.
    pub fn leave(&mut self, index: usize) {
        if index >= self.clients.len() {
            return;
        }
        let mut client = self.clients.remove(index);
        self.left
            .push((Peer::Client(index), client.sim.take_output()));
        self.server.remove_player(client.player);
    }

    /// Stop: everyone leaves (`PlayerRemoving` on the server), then the
    /// server closes (`BindToClose`), so what a game saves on the way out
    /// is saved, as when a Studio test stops.
    pub fn shutdown(&mut self) {
        while !self.clients.is_empty() {
            self.leave(self.clients.len() - 1);
        }
        self.server.step();
        self.server.close();
    }

    /// Runs frames until `seconds` of game time have passed.
    pub fn run_for(&mut self, seconds: f64) {
        let frames = (seconds * crate::FRAME_HZ).round() as u64;
        for _ in 0..frames {
            self.step();
        }
    }

    pub fn time(&self) -> f64 {
        self.server.time()
    }

    /// The server's or a client's simulation.
    pub fn sim(&self, peer: Peer) -> Option<&Simulation> {
        match peer {
            Peer::Server => Some(&self.server),
            Peer::Client(i) => self.clients.get(i).map(|c| &c.sim),
        }
    }

    /// The DataStores, purchases and web lookups of every peer, the
    /// clients that join later too. Call before `start`.
    pub fn set_services(&mut self, config: rbx_lua::server::Services) {
        self.server.set_services(config.clone());
        for client in &mut self.clients {
            client.sim.set_services(config.clone());
        }
        self.services = Some(config);
    }

    /// The teleport GUI player `user_id` brings from the run they left
    /// (with their `arrivals` entry in the services): call before they
    /// join.
    pub fn set_arriving_gui(&mut self, user_id: i64, gui: WeakDom) {
        self.arriving.insert(user_id, gui);
    }

    /// Every teleport a script asked for since the last call, the
    /// server's and the clients'.
    pub fn take_teleports(&mut self) -> Vec<Teleport> {
        let peers = std::iter::once(&self.server).chain(self.clients.iter().map(|c| &c.sim));
        let mut out = Vec::new();
        for sim in peers {
            for request in sim.take_teleports() {
                let gui = request.gui.and_then(|gui| {
                    let mut copy = WeakDom::new();
                    copy_subtree(&sim.dom(), gui, &mut copy, None)?;
                    Some(copy)
                });
                out.push(Teleport { request, gui });
            }
        }
        out
    }

    /// Every toast a script asked the core GUI for since the last call.
    pub fn take_notices(&mut self) -> Vec<rbx_lua::server::Notice> {
        let mut out = self.server.take_notices();
        for client in &self.clients {
            out.extend(client.sim.take_notices());
        }
        out
    }

    /// Every purchase prompt a script asked for since the last call, the
    /// server's and the clients'.
    pub fn take_prompts(&mut self) -> Vec<rbx_lua::server::PurchasePrompt> {
        let mut out = self.server.take_prompts();
        for client in &self.clients {
            out.extend(client.sim.take_prompts());
        }
        out
    }

    /// The test purchase dialog for `prompt` closed: the server (ownership,
    /// `ProcessReceipt`) and the buyer's client hear it.
    pub fn purchase_finished(&mut self, prompt: &rbx_lua::server::PurchasePrompt, bought: bool) {
        self.server.purchase_finished(prompt, bought);
        if let Some(client) = self.clients.iter_mut().find(|c| c.player == prompt.player) {
            client.sim.purchase_finished(prompt, bought);
        }
    }

    pub fn sim_mut(&mut self, peer: Peer) -> Option<&mut Simulation> {
        match peer {
            Peer::Server => Some(&mut self.server),
            Peer::Client(i) => self.clients.get_mut(i).map(|c| &mut c.sim),
        }
    }

    /// Queues what local player `index` did, for their client's next frame.
    pub fn input(&mut self, index: usize, input: Input) {
        if let Some(client) = self.clients.get_mut(index) {
            client.sim.input(input);
        }
    }

    /// What every peer printed since the last call, server first.
    pub fn take_output(&mut self) -> Vec<(Peer, Output)> {
        let mut out = vec![(Peer::Server, self.server.take_output())];
        out.append(&mut self.left);
        for (i, client) in self.clients.iter_mut().enumerate() {
            out.push((Peer::Client(i), client.sim.take_output()));
        }
        out
    }

    /// The script timeout on the server and every client, those to come
    /// too (see [`Simulation::set_script_timeout`]).
    pub fn set_script_timeout(&mut self, timeout: std::time::Duration) {
        self.server.set_script_timeout(timeout);
        for client in &mut self.clients {
            client.sim.set_script_timeout(timeout);
        }
        self.script_timeout = Some(timeout);
    }

    pub fn set_fetcher(&mut self, fetcher: std::sync::Arc<dyn rbx_assets::AssetFetcher>) {
        self.server.set_fetcher(fetcher.clone());
        for client in &mut self.clients {
            client.sim.set_fetcher(fetcher.clone());
        }
        self.fetcher = Some(fetcher);
    }

    /// Where avatars come from, on the server and every client (a
    /// LocalScript may ask for a description and dress its own dummy).
    pub fn set_avatars(&mut self, avatars: std::rc::Rc<dyn crate::players::Avatars>) {
        self.server.set_avatars(avatars.clone());
        for client in &mut self.clients {
            client.sim.set_avatars(avatars.clone());
        }
        self.avatars = Some(avatars);
    }
}

impl Client {
    /// The place reaches a new client: every replicated service's
    /// contents, then `game.Loaded`, then its `PlayerScripts`.
    fn arrive(&mut self, server: &Simulation) {
        self.sim.set_remote(true);
        self.arrive_all(server);
        self.sim.set_remote(false);
    }

    fn arrive_all(&mut self, server: &Simulation) {
        let roots: Vec<Ref> = {
            let dom = server.dom();
            dom.root_refs()
                .iter()
                .copied()
                .filter(|r| {
                    dom.get(*r).is_some_and(|i| {
                        !SERVER_ONLY.contains(&i.class()) && i.class() != "ReplicatedFirst"
                    })
                })
                .flat_map(|r| {
                    dom.get(r)
                        .map(|i| i.children().to_vec())
                        .unwrap_or_default()
                })
                .collect()
        };
        let own = match server
            .dom()
            .get(self.player)
            .map(|p| p.properties().get("Character"))
        {
            Some(Some(Variant::Ref(c))) if server.dom().in_tree(*c) => Some(*c),
            _ => None,
        };
        let roots: Vec<Ref> = roots.into_iter().filter(|r| Some(*r) != own).collect();
        self.deferred = own;
        // The whole snapshot lands before its LocalScripts start: one in
        // PlayerGui finds ReplicatedStorage's contents.
        self.sim.hold_scripts();
        for root in roots {
            self.deliver(server, &Op::Tree(root, None));
        }
        self.loaded = true;
        self.sim.set_loaded();
        self.sim.release_scripts();
        if self.sim.dom().get(self.player).is_some() {
            let db = self.sim.db();
            crate::players::player_scripts(&mut self.sim.dom_mut(), db, self.player);
            self.sim.flush();
        }
    }

    /// Works out what of Workspace the client should have now: what to
    /// stream in (parents before children, persistent models first, then
    /// nearest first; `true` for a persistent one) and out. Answers the
    /// `RequestStreamAroundAsync` calls that came in.
    fn plan_stream(&mut self, server: &Simulation) -> (Vec<(Ref, bool)>, Vec<Ref>) {
        let Some(stream) = self.stream.as_mut() else {
            return Default::default();
        };
        let frame = server.frame();
        let requests = std::mem::take(&mut stream.requests);
        let full = frame >= stream.due;
        if !full && requests.is_empty() {
            return Default::default();
        }
        let first = !stream.started;
        let sdom = server.dom();
        let characters = streaming::characters(&sdom);
        let own = match sdom
            .get(self.player)
            .map(|p| p.properties().get("Character"))
        {
            Some(Some(Variant::Ref(c))) => Some(*c),
            _ => None,
        };
        let units = streaming::units(&sdom, server.db(), self.player, &characters, &|model| {
            server.persistent_players(model)
        });
        let settings = stream.settings;
        let foci = streaming::foci(&sdom, self.player, &server.replication_foci(self.player));
        if full {
            // Right after the join only the minimum radius comes; the rest
            // of the target radius a moment later.
            stream.due = frame + if first { STREAM_FIRST } else { STREAM_EVERY };
            stream.started = true;
            // The integrity pause: the minimum radius around the character
            // isn't all here (what this look streams in lands for the next
            // one).
            stream.paused = !first
                && settings.integrity != Integrity::Disabled
                && streaming::character_root(&sdom, self.player).is_some_and(|f| {
                    units.iter().any(|u| {
                        u.kind == Kind::Spatial
                            && !stream.have.contains(&u.root)
                            && u.reach(f) <= settings.min
                    })
                });
            stream.recent.retain(|(_, until)| *until > frame);
            stream
                .recent
                .extend(foci.iter().map(|f| (*f, frame + STREAM_KEEP)));
        }
        // An area asked for comes in at once and is kept like the focus's
        // minimum radius for a while.
        stream
            .recent
            .extend(requests.iter().map(|r| (r.position, frame + STREAM_KEEP)));
        let mut areas: Vec<([f32; 3], f32)> = stream
            .recent
            .iter()
            .map(|(p, _)| (*p, settings.min))
            .collect();
        if full {
            let radius = if first { settings.min } else { settings.target };
            areas.extend(foci.iter().map(|f| (*f, radius)));
        }
        let unit_set: HashSet<Ref> = units.iter().map(|u| u.root).collect();
        // A unit's ancestors that are units themselves (parts holding
        // parts): they come before it and stay while it does.
        let chain = |r: Ref| {
            let mut up = Vec::new();
            let mut at = sdom.parent(r);
            while let Some(p) = at {
                if unit_set.contains(&p) {
                    up.push(p);
                }
                at = sdom.parent(p);
            }
            up.reverse();
            up
        };
        let mut wanted: Vec<(f32, Ref, bool)> = Vec::new();
        let mut staying = HashSet::new();
        for unit in &units {
            let persistent = unit.kind == Kind::Persistent;
            let reach = areas
                .iter()
                .map(|(c, radius)| (unit.reach(*c), *radius))
                .filter(|(d, radius)| d <= radius)
                .map(|(d, _)| d)
                .fold(f32::INFINITY, f32::min);
            let has = stream.have.contains(&unit.root);
            if Some(unit.root) == own {
                // Its own character always; on the join it lands by itself.
                if !has && first {
                    stream.have.insert(unit.root);
                } else if !has {
                    wanted.push((-1.0, unit.root, false));
                }
                staying.insert(unit.root);
            } else if (persistent && full) || reach.is_finite() {
                if !has {
                    wanted.push((if persistent { -2.0 } else { reach }, unit.root, persistent));
                }
                staying.insert(unit.root);
            } else if has
                && (!full
                    || !settings.opportunistic
                    || persistent
                    || foci.is_empty()
                    || foci.iter().any(|f| unit.reach(*f) <= settings.target))
            {
                staying.insert(unit.root);
            }
        }
        // What the server took away goes by the server's own changes.
        stream.have.retain(|r| unit_set.contains(r));
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut order = Vec::new();
        for (_, root, persistent) in wanted {
            for r in chain(root) {
                if stream.have.insert(r) {
                    order.push((r, persistent));
                }
            }
            if stream.have.insert(root) {
                order.push((root, persistent));
            }
        }
        let pinned: HashSet<Ref> = staying.iter().flat_map(|r| chain(*r)).collect();
        let out: Vec<Ref> = stream
            .have
            .iter()
            .copied()
            .filter(|r| !staying.contains(r) && !pinned.contains(r))
            .collect();
        for r in &out {
            stream.have.remove(r);
        }
        for request in requests {
            server.stream_request_done(request.id);
        }
        (order, out)
    }

    /// One look at what to stream in and out after the server's frame.
    fn stream(&mut self, server: &Simulation) {
        let (wanted, out) = self.plan_stream(server);
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        let paused = stream.paused;
        let was = matches!(
            self.sim
                .dom()
                .get(self.player)
                .and_then(|p| p.properties().get("GameplayPaused")),
            Some(Variant::Bool(true))
        );
        if was != paused {
            let _ = self.sim.dom_mut().set_property(
                self.player,
                "GameplayPaused",
                Variant::Bool(paused),
            );
            self.sim.flush();
        }
        for (root, persistent) in &wanted {
            if !persistent {
                self.persistent_loaded();
            }
            self.stream_in(server, *root);
        }
        if self.stream.as_ref().is_some_and(|s| s.started) {
            self.persistent_loaded();
        }
        for root in out {
            // Streamed out: parented to nil, not destroyed.
            if self.sim.dom().in_tree(root) {
                self.sim.dom_mut().unparent(root);
                self.sim.flush();
            }
        }
    }

    /// `Workspace.PersistentLoaded`, once, after the persistent models
    /// arrived on the join.
    fn persistent_loaded(&mut self) {
        let Some(stream) = self.stream.as_mut().filter(|s| !s.persistent_loaded) else {
            return;
        };
        stream.persistent_loaded = true;
        let workspace = {
            let dom = self.sim.dom();
            dom.root_refs()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        };
        if let Some(workspace) = workspace {
            self.sim.fire_signal(
                workspace,
                "PersistentLoaded",
                vec![Arg::Instance(self.player)],
            );
        }
    }

    /// Unit `root` streams in. One that streamed out earlier is still the
    /// same object (held references see it come back) with the server's
    /// values again: what this client changed on it is lost.
    fn stream_in(&mut self, server: &Simulation, root: Ref) {
        let back = self.sim.dom().get(root).is_some() && !self.sim.dom().in_tree(root);
        if back {
            let sdom = server.dom();
            let mut stack = vec![root];
            let mut dom = self.sim.dom_mut();
            while let Some(at) = stack.pop() {
                let (Some(source), Some(target)) = (sdom.get(at), dom.get(at)) else {
                    continue;
                };
                stack.extend(target.children().iter().copied());
                let changed: Vec<(String, Variant)> = source
                    .properties()
                    .iter()
                    .filter(|(k, v)| target.properties().get(k.as_str()) != Some(*v))
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect();
                let dropped: Vec<String> = target
                    .properties()
                    .keys()
                    .filter(|k| !source.properties().contains_key(k.as_str()))
                    .map(|k| k.to_string())
                    .collect();
                for (key, value) in changed {
                    let _ = dom.set_property(at, &key, value);
                }
                for key in dropped {
                    let _ = dom.remove_property(at, &key);
                }
            }
        }
        self.deliver(server, &Op::Tree(root, None));
        if back {
            // What was added under it on the server meanwhile.
            let children: Vec<Ref> = server
                .dom()
                .get(root)
                .map(|i| i.children().to_vec())
                .unwrap_or_default();
            for child in children {
                if self.sim.dom().get(child).is_none() {
                    self.deliver(server, &Op::Tree(child, None));
                }
            }
        }
    }

    /// Whether streaming keeps `r` from this client for now.
    fn streamed_out(
        &self,
        sdom: &WeakDom,
        db: &rbx_reflection::ReflectionDatabase,
        r: Ref,
        characters: &HashSet<Ref>,
    ) -> bool {
        self.stream.as_ref().is_some_and(|s| {
            streaming::unit_of(sdom, db, r, characters).is_some_and(|u| !s.have.contains(&u))
        })
    }

    /// One replicated change or packet.
    fn deliver(&mut self, server: &Simulation, op: &Op) {
        // What the server's frame moves is the server's doing; a remote's
        // handler moving a tool is the player's.
        self.sim.set_remote(!matches!(op, Op::Packet(_)));
        self.deliver_op(server, op);
        self.sim.set_remote(false);
    }

    fn deliver_op(&mut self, server: &Simulation, op: &Op) {
        match op {
            Op::Property(r, name) => self.property(server, *r, name),
            Op::Tree(r, parent) => self.tree(server, *r, *parent),
            // Destroyed: it leaves its parent (`ChildRemoved`), then goes.
            // A player leaving is `PlayerRemoving` first.
            Op::Removed(r) => {
                let players = {
                    let dom = self.sim.dom();
                    dom.get(*r)
                        .filter(|i| i.class() == "Player")
                        .and_then(|_| dom.parent(*r))
                };
                if let Some(players) = players {
                    self.sim
                        .fire_signal(players, "PlayerRemoving", vec![Arg::Instance(*r)]);
                }
                if self.sim.dom().in_tree(*r) {
                    self.sim.dom_mut().unparent(*r);
                    self.sim.flush();
                }
                if self.sim.dom().get(*r).is_some() {
                    self.sim.destroy(*r);
                }
            }
            Op::Packet(out) => {
                let mine = match out.to {
                    To::Player(p) => p == self.player,
                    To::AllPlayers => true,
                    To::Server => false,
                };
                if mine {
                    self.sim.receive(None, out.packet.clone());
                }
            }
        }
        self.characters_arrived();
    }

    fn property(&mut self, server: &Simulation, r: Ref, name: &str) {
        let sdom = server.dom();
        if !in_scope(&sdom, r, self.player) {
            return;
        }
        let Some(source) = sdom.get(r) else {
            return;
        };
        // What physics moves arrives unlogged (`place_parts`); a logged
        // move of a part this client simulates is the server's script
        // teleporting it (`PivotTo` on the player's character), which the
        // owner takes.
        let character = (source.class() == "Player" && name == "Character").then(|| {
            let old = match self
                .sim
                .dom()
                .get(r)
                .map(|i| i.properties().get("Character"))
            {
                Some(Some(Variant::Ref(c))) => Some(*c),
                _ => None,
            };
            let new = match source.properties().get("Character") {
                Some(Variant::Ref(c)) if sdom.get(*c).is_some() => Some(*c),
                _ => None,
            };
            (old, new)
        });
        {
            let mut dom = self.sim.dom_mut();
            let Some(target) = dom.get(r) else {
                return;
            };
            if name == "Name" {
                if target.name() != source.name() {
                    let _ = dom.set_name(r, source.name());
                }
            } else {
                match source.properties().get(name) {
                    Some(value) if target.properties().get(name) != Some(value) => {
                        let _ = dom.set_property(r, name, value.clone());
                    }
                    None if target.properties().contains_key(name) => {
                        let _ = dom.remove_property(r, name);
                    }
                    _ => return,
                }
            }
        }
        drop(sdom);
        if let Some((old, new)) = character {
            if let Some(old) = old.filter(|o| Some(*o) != new) {
                if self.sim.dom().get(old).is_some() {
                    self.sim
                        .fire_signal(r, "CharacterRemoving", vec![Arg::Instance(old)]);
                }
            }
            if let Some(new) = new {
                self.characters.push((r, new));
            }
        }
        self.sim.flush();
    }

    /// An instance added or moved on the server: copied in if it is now in
    /// the client's scope and missing, moved if present, or taken out (to
    /// nil) if it left the scope.
    fn tree(&mut self, server: &Simulation, r: Ref, given: Option<Ref>) {
        let sdom = server.dom();
        if sdom.get(r).is_none() {
            // Made and destroyed within the frame: it still arrives (by
            // class and name), for the removal that follows to take away.
            let fleeting = server.tombstone(r).zip(given).filter(|(_, p)| {
                in_scope(&sdom, *p, self.player) && self.sim.dom().get(*p).is_some()
            });
            drop(sdom);
            if let Some(((class, name), parent)) = fleeting {
                if self.sim.dom().get(r).is_none() {
                    let mut dom = self.sim.dom_mut();
                    dom.insert(rbx_dom::Instance::new(r, class, name));
                    dom.set_parent(r, Some(parent));
                }
                self.sim.flush();
            }
            return;
        }
        let scope = in_scope(&sdom, r, self.player);
        let present = self.sim.dom().get(r).is_some();
        if !scope {
            if present && self.sim.dom().in_tree(r) {
                self.sim.dom_mut().unparent(r);
                drop(sdom);
                self.sim.flush();
            }
            return;
        }
        let parent = sdom.parent(r);
        // The client's own Player was there from the start (`first_view`):
        // its contents still arrive with the place, and it still arrives.
        let own_player = r == self.player && !self.loaded;
        if present && !own_player {
            let here = self.sim.dom().parent(r);
            if here != parent && parent.is_some_and(|p| self.sim.dom().get(p).is_some()) {
                self.sim.dom_mut().set_parent(r, parent);
                drop(sdom);
                self.sim.flush();
            }
            return;
        }
        if parent.is_some_and(|p| self.sim.dom().get(p).is_none()) {
            return;
        }
        // One instance at a time, depth first, each one's events fired
        // before the next arrives.
        let characters = if self.stream.is_some() {
            streaming::characters(&sdom)
        } else {
            HashSet::new()
        };
        let mut order = Vec::new();
        let mut stack = vec![r];
        while let Some(at) = stack.pop() {
            let Some(source) = sdom.get(at) else {
                continue;
            };
            if !in_scope(&sdom, at, self.player)
                || replicates_not(&sdom, at)
                || self.streamed_out(&sdom, server.db(), at, &characters)
            {
                continue;
            }
            order.push((
                at,
                source.class().to_owned(),
                source.name().to_owned(),
                sdom.parent(at),
                source.properties().clone(),
            ));
            stack.extend(source.children().iter().rev().copied());
        }
        drop(sdom);
        // The join's snapshot lands whole before any of its events run, so
        // a ReplicatedFirst script waiting on a module finds the module's
        // children there too. Later additions arrive one instance at a
        // time (the ReplicationOrder golden).
        let whole = !self.loaded;
        let mut players = Vec::new();
        let outer = self.sim.holding_scripts();
        self.sim.hold_scripts();
        for (at, class, name, parent, properties) in order {
            if self.sim.dom().get(at).is_some() {
                if at == self.player && own_player {
                    // What the server set on it since the join (its
                    // `Character`) comes with it.
                    let mut dom = self.sim.dom_mut();
                    for (key, value) in properties {
                        let _ = dom.set_property(at, &key, value);
                    }
                    drop(dom);
                    players.push(at);
                }
                continue;
            }
            {
                let mut dom = self.sim.dom_mut();
                let mut copy = rbx_dom::Instance::new(at, class.as_str(), name.as_str());
                *copy.properties_mut() = properties;
                dom.insert(copy);
                if let Some(parent) = parent.filter(|p| dom.get(*p).is_some()) {
                    dom.set_parent(at, Some(parent));
                }
            }
            if class == "Player" {
                players.push(at);
            }
            if !whole {
                self.sim.flush();
                for player in players.drain(..) {
                    self.player_arrived(player);
                }
            }
        }
        if whole {
            self.sim.flush();
            for player in players {
                self.player_arrived(player);
            }
        }
        if !outer {
            self.sim.release_scripts();
        }
    }

    fn player_arrived(&mut self, at: Ref) {
        let players = self.sim.dom().parent(at);
        if let Some(players) = players {
            self.sim
                .fire_signal(players, "PlayerAdded", vec![Arg::Instance(at)]);
        }
        let character = match self
            .sim
            .dom()
            .get(at)
            .map(|i| i.properties().get("Character"))
        {
            Some(Some(Variant::Ref(c))) => Some(*c),
            _ => None,
        };
        if let Some(character) = character {
            self.characters.push((at, character));
        }
    }

    /// `CharacterAdded` (and `CharacterAppearanceLoaded`) for characters
    /// that are here now.
    fn characters_arrived(&mut self) {
        let ready: Vec<(Ref, Ref)> = {
            let dom = self.sim.dom();
            self.characters
                .iter()
                .copied()
                .filter(|(_, c)| dom.in_tree(*c))
                .collect()
        };
        if ready.is_empty() {
            return;
        }
        self.characters.retain(|x| !ready.contains(x));
        for (player, character) in ready {
            self.sim
                .fire_signal(player, "CharacterAdded", vec![Arg::Instance(character)]);
            self.sim.fire_signal(
                player,
                "CharacterAppearanceLoaded",
                vec![Arg::Instance(character)],
            );
        }
    }

    /// What the client sends the server after its frame.
    fn collect(&mut self) {
        let packets = self.sim.take_outgoing();
        let parts: Vec<_> = {
            let owned = self.sim.ownership_mut().owned.clone();
            self.sim
                .moved_parts_since(&mut self.moving)
                .into_iter()
                .filter(|placed| owned.contains(&placed.part))
                .collect()
        };
        self.overruled = self.sim.take_overruled();
        let states = self
            .sim
            .humanoid_states()
            .into_iter()
            .filter(|(_, _, mine)| *mine)
            .map(|(h, s, _)| (h, s))
            .collect();
        let tracks = self
            .sim
            .take_track_ops()
            .into_iter()
            .filter(|(origin, _)| origin.is_none())
            .map(|(_, op)| op)
            .collect();
        let tools = self.sim.take_tool_ops();
        self.outbox = Upstream {
            packets,
            parts,
            states,
            tracks,
            tools,
        };
    }
}

/// The server's frame as one ordered list: its changes (each property once,
/// where it first changed) with its reliable packets where they were sent,
/// then the unreliable ones.
fn frame_ops(stream: Vec<(u64, Change)>, packets: Vec<Outgoing>) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut seen: HashSet<(Ref, String)> = HashSet::new();
    let (reliable, unreliable): (Vec<_>, Vec<_>) = packets.into_iter().partition(|o| {
        !matches!(
            o.packet,
            Packet::Event {
                unreliable: true,
                ..
            }
        )
    });
    let mut reliable = reliable.into_iter().peekable();
    for (at, change) in stream {
        while reliable.peek().is_some_and(|o| o.at < at) {
            ops.extend(reliable.next().map(Op::Packet));
        }
        match change {
            Change::Property { referent, name } => {
                if seen.insert((referent, name.clone())) {
                    ops.push(Op::Property(referent, name));
                }
            }
            Change::Added(r) => ops.push(Op::Tree(r, None)),
            Change::Parent { referent, new, .. } => ops.push(Op::Tree(referent, new)),
            Change::Removed(r) => ops.push(Op::Removed(r)),
            Change::Class(_) => {}
        }
    }
    ops.extend(reliable.map(Op::Packet));
    ops.extend(unreliable.into_iter().map(Op::Packet));
    ops
}

/// Whether `r` replicates to `player`'s client where it stands: in the
/// tree, in no server-only service, in no other player's `PlayerGui`.
fn in_scope(dom: &WeakDom, r: Ref, player: Ref) -> bool {
    if !dom.in_tree(r) {
        return false;
    }
    let mut at = r;
    loop {
        let Some(instance) = dom.get(at) else {
            return false;
        };
        match dom.parent(at) {
            None => return !SERVER_ONLY.contains(&instance.class()),
            Some(parent) => {
                if instance.class() == "PlayerGui" && parent != player {
                    return false;
                }
                at = parent;
            }
        }
    }
}

/// Instances a client keeps its own of: the server's cameras.
fn replicates_not(dom: &WeakDom, r: Ref) -> bool {
    dom.get(r).is_some_and(|i| i.class() == "Camera")
}

/// What a client has before the place arrives: the services with their
/// properties, `ReplicatedFirst`'s contents, and its own `Player` (bare):
/// a ReplicatedFirst script finds `LocalPlayer` there.
fn first_view(server: &WeakDom, player: Ref) -> WeakDom {
    let mut dom = server.clone();
    let unparented: Vec<Ref> = dom.unparented_refs().collect();
    for r in unparented {
        dom.remove(r);
    }
    for root in dom.root_refs().to_vec() {
        let keep = dom
            .get(root)
            .is_some_and(|i| i.class() == "ReplicatedFirst");
        if keep {
            continue;
        }
        for child in dom
            .get(root)
            .map(|i| i.children().to_vec())
            .unwrap_or_default()
        {
            if child != player {
                dom.remove(child);
            }
        }
    }
    for child in dom
        .get(player)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
    {
        dom.remove(child);
    }
    dom.take_changes();
    dom
}
