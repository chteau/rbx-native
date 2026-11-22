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
//! - all of it crosses a simulated link ([`crate::link`]), instant by
//!   default: what a frame sends is snapshotted as it leaves, so a client
//!   behind a slow link sees the server's world as it was.

use std::collections::{BTreeMap, HashMap, HashSet};

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, Outgoing, Packet, To};

use crate::animation::TrackOp;
use crate::humanoid::State;
use crate::link::{Meter, Network, Pipe};
use crate::sim::{Placed, Simulation};
use crate::{Input, Output, FRAME_HZ};

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
    /// The place's snapshot has left the server (it may still be on the
    /// way).
    sent_place: bool,
    /// Server to client and client to server.
    down: Pipe<Down>,
    up: Pipe<Up>,
    /// Poses a lost physics update carried, for the next one to take.
    carry_down: Vec<Placed>,
    carry_up: Vec<Placed>,
    /// The frame of the newest pose applied per part, at each end: an
    /// older one arriving late is dropped.
    newest_down: HashMap<Ref, u64>,
    newest_up: HashMap<Ref, u64>,
    /// `UnreliableRemoteEvent` packets the server took from this client in
    /// one second of frames: (second, count).
    burst: (u64, u32),
    /// Bytes each way, for `Stats`: data and physics.
    meters: [Meter; 4],
    /// Parts it moved last frame, to say when they stop.
    moving: HashSet<Ref>,
    /// Parts the server simulates that it moved itself: the server's pose
    /// puts them back next frame, asleep or not (Ownership
    /// `client_view_server_owned`).
    overruled: Vec<Ref>,
}

/// `meters` indexes.
const DOWN_DATA: usize = 0;
const DOWN_PHYSICS: usize = 1;
const UP_DATA: usize = 2;
const UP_PHYSICS: usize = 3;

/// `UnreliableRemoteEvent` packets a server takes from one client a second
/// (creator-docs: "approximately 500 requests per second, per client");
/// the rest are dropped.
const UNRELIABLE_PER_SECOND: u32 = 500;

/// What crosses the link from the server to one client, each frame in
/// this order.
enum Down {
    /// The place, for a client that just joined.
    Place(Vec<Snap>),
    /// The frame's changes and reliable packets.
    Ops(Vec<Snap>),
    Unreliable(Packet),
    /// Poses from the server's frame `.0`.
    Physics(u64, Vec<Placed>),
    Rest {
        states: Vec<(Ref, State)>,
        tracks: Vec<TrackOp>,
        forwarded: Vec<(Ref, String, Vec<Arg>)>,
    },
}

/// What crosses the link from a client to the server, each frame in this
/// order.
enum Up {
    Packets(Vec<Packet>),
    Unreliable(Packet),
    Physics(u64, Vec<Placed>),
    Rest {
        states: Vec<(Ref, State)>,
        tracks: Vec<TrackOp>,
        tools: Vec<crate::tools::ClientOp>,
    },
}

/// One replicated change as it left the server for one client: what the
/// server had then, whatever the server does next.
enum Snap {
    /// `value` `None`: the property was cleared. `character`: a `Player`'s
    /// `Character` changed, to this.
    Property {
        r: Ref,
        name: String,
        value: Option<Variant>,
        character: Option<Option<Ref>>,
    },
    /// Made and destroyed within the frame: it still arrives (by class and
    /// name), for the removal that follows to take away.
    Fleeting {
        r: Ref,
        class: String,
        name: String,
        parent: Ref,
    },
    /// Out of the client's scope now: taken out (to nil).
    Leave(Ref),
    /// In scope under `parent`: moved there if the client has it, else
    /// copied in from `nodes` (when the client lacked it as it left).
    Place {
        r: Ref,
        parent: Option<Ref>,
        nodes: Option<Vec<Node>>,
    },
    Removed(Ref),
    Packet(Packet),
}

/// An instance being copied in: (referent, class, name, parent,
/// properties).
type Node = (Ref, String, String, Option<Ref>, BTreeMap<String, Variant>);

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
    network: Network,
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
        // Clients copy the server's `Stats`, so their own items under it
        // never meet a second one replicated later.
        stats_service(&mut server.dom_mut());
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
            network: Network::default(),
        })
    }

    /// The link every client has to the server, now and for those who join
    /// later; `Player:GetNetworkPing()` and `Stats` report it at once.
    pub fn set_network(&mut self, network: Network) {
        self.network = network;
        self.server.set_network_ping(network.ping());
        for client in &mut self.clients {
            client.sim.set_network_ping(network.ping());
        }
    }

    pub fn network(&self) -> Network {
        self.network
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
        let mut sim = Simulation::new(first_view(&self.server.dom(), player))?;
        sim.make_client(index, player);
        network_items(&mut sim.dom_mut());
        sim.set_network_ping(self.network.ping());
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
        self.clients.push(Client {
            sim,
            player,
            name: name.into(),
            loaded: false,
            characters: Vec::new(),
            deferred: None,
            sent_place: false,
            down: Pipe::new(2 * index as u64 + 1),
            up: Pipe::new(2 * index as u64 + 2),
            carry_down: Vec::new(),
            carry_up: Vec::new(),
            newest_down: HashMap::new(),
            newest_up: HashMap::new(),
            burst: (0, 0),
            meters: Default::default(),
            moving: HashSet::new(),
            overruled: Vec::new(),
        });
        Ok(index)
    }

    /// One frame: what the clients sent arrives at the server, the server
    /// steps, then each client gets the server's frame and steps.
    pub fn step(&mut self) {
        self.relayed.clear();
        let now = self.server.frame();
        for client in &mut self.clients {
            let player = client.player;
            for up in client.up.due(now) {
                match up {
                    Up::Packets(packets) => {
                        for packet in packets {
                            self.server.receive(Some(player), packet);
                        }
                    }
                    Up::Unreliable(packet) => {
                        let second = now / FRAME_HZ as u64;
                        if client.burst.0 != second {
                            client.burst = (second, 0);
                        }
                        client.burst.1 += 1;
                        if client.burst.1 <= UNRELIABLE_PER_SECOND {
                            self.server.receive(Some(player), packet);
                        }
                    }
                    Up::Physics(frame, parts) => {
                        let owned = self.server.ownership_mut().owned_by(player);
                        let parts: Vec<_> = fresh(&mut client.newest_up, frame, parts)
                            .into_iter()
                            .filter(|placed| owned.contains(&placed.part))
                            .collect();
                        self.server.place_parts(&parts);
                        self.relayed.extend(parts);
                    }
                    Up::Rest {
                        states,
                        tracks,
                        tools,
                    } => {
                        for (humanoid, state) in states {
                            self.server.set_humanoid_state(humanoid, state);
                        }
                        for op in tracks {
                            self.server.mirror_track(Some(player), op);
                        }
                        self.server.apply_tool_ops(player, tools);
                    }
                }
            }
        }

        self.server.step();

        let sent = self.server.frame();
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

        let network = self.network;
        let rtt = network.ping();
        let server = &self.server;
        for client in &mut self.clients {
            let owned = server.ownership_mut().owned_by(client.player);
            client.sim.ownership_mut().owned = owned;
            client.sim.sync_collision_groups(server);
            let mut unreliable = Vec::new();
            let frame = if client.sent_place {
                let mut snaps = Vec::new();
                if let Some(character) = client.deferred.take() {
                    let parent = server.dom().parent(character);
                    if parent.is_some() {
                        snaps.extend(client.snap(server, &Op::Tree(character, parent)));
                    }
                }
                for op in &ops {
                    match client.snap(server, op) {
                        Some(Snap::Packet(packet)) if unreliable_event(&packet) => {
                            unreliable.push(packet);
                        }
                        snap => snaps.extend(snap),
                    }
                }
                Down::Ops(snaps)
            } else {
                client.sent_place = true;
                Down::Place(client.snap_place(server))
            };
            if let Down::Place(snaps) | Down::Ops(snaps) = &frame {
                client.meters[DOWN_DATA].add(sent, snaps.iter().map(snap_size).sum());
            }
            client.down.reliable(sent, &network.down, rtt, frame);
            for packet in unreliable {
                let bytes = packet_size(&packet);
                let down = Down::Unreliable(packet);
                if client.down.unreliable(sent, &network.down, down).is_ok() {
                    client.meters[DOWN_DATA].add(sent, bytes);
                }
            }
            let mut poses = std::mem::take(&mut client.carry_down);
            merge(&mut poses, moved.iter().copied());
            let overruled = server.poses(&std::mem::take(&mut client.overruled));
            merge(&mut poses, overruled);
            client.meters[DOWN_PHYSICS].add(sent, poses.len() * POSE_BYTES);
            let physics = Down::Physics(sent, poses);
            if let Err(Down::Physics(_, lost)) =
                client.down.unreliable(sent, &network.down, physics)
            {
                client.carry_down = lost;
            }
            let rest = Down::Rest {
                states: states.clone(),
                tracks: tracks
                    .iter()
                    .filter(|(origin, _)| *origin != Some(client.player))
                    .map(|(_, op)| op.clone())
                    .collect(),
                forwarded: forwarded.clone(),
            };
            client.down.reliable(sent, &network.down, rtt, rest);

            for frame in client.down.due(sent) {
                client.apply(frame);
            }
            client.sim.step();
            client.collect(sent, &network);
        }
        self.meter(sent);

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

    /// `Stats`' data and physics rates on every peer, over the last second:
    /// a client receives what goes down and sends what goes up, the server
    /// the other way round, for all its clients.
    fn meter(&mut self, now: u64) {
        let mut server = [0.0; 4];
        for client in &mut self.clients {
            let rates = client.meters.each_mut().map(|m| m.kbps(now));
            for (total, rate) in server.iter_mut().zip(rates) {
                *total += rate;
            }
            let [down, down_physics, up, up_physics] = rates;
            write_rates(&client.sim, [down, up, down_physics, up_physics]);
        }
        let [down, down_physics, up, up_physics] = server;
        write_rates(&self.server, [up, down, up_physics, down_physics]);
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
    /// The place as it reaches a new client: every replicated service's
    /// contents (its own character a frame later).
    fn snap_place(&mut self, server: &Simulation) -> Vec<Snap> {
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
        self.deferred = own;
        roots
            .into_iter()
            .filter(|r| Some(*r) != own)
            .filter_map(|root| self.snap(server, &Op::Tree(root, None)))
            .collect()
    }

    /// `op` as it leaves the server for this client; `None` when it has
    /// nothing for it.
    fn snap(&self, server: &Simulation, op: &Op) -> Option<Snap> {
        match op {
            Op::Property(r, name) => self.snap_property(server, *r, name),
            Op::Tree(r, given) => self.snap_tree(server, *r, *given),
            Op::Removed(r) => Some(Snap::Removed(*r)),
            Op::Packet(out) => {
                let mine = match out.to {
                    To::Player(p) => p == self.player,
                    To::AllPlayers => true,
                    To::Server => false,
                };
                mine.then(|| Snap::Packet(out.packet.clone()))
            }
        }
    }

    fn snap_property(&self, server: &Simulation, r: Ref, name: &str) -> Option<Snap> {
        let sdom = server.dom();
        if !in_scope(&sdom, r, self.player) {
            return None;
        }
        let source = sdom.get(r)?;
        let value = if name == "Name" {
            Some(Variant::String(source.name().to_owned()))
        } else {
            source.properties().get(name).cloned()
        };
        let character = (source.class() == "Player" && name == "Character").then(|| {
            match source.properties().get("Character") {
                Some(Variant::Ref(c)) if sdom.get(*c).is_some() => Some(*c),
                _ => None,
            }
        });
        Some(Snap::Property {
            r,
            name: name.to_owned(),
            value,
            character,
        })
    }

    /// An instance added or moved on the server, with its subtree copied
    /// out when the client doesn't have it yet.
    fn snap_tree(&self, server: &Simulation, r: Ref, given: Option<Ref>) -> Option<Snap> {
        let sdom = server.dom();
        if sdom.get(r).is_none() {
            let ((class, name), parent) = server
                .tombstone(r)
                .zip(given)
                .filter(|(_, p)| in_scope(&sdom, *p, self.player))?;
            return Some(Snap::Fleeting {
                r,
                class,
                name,
                parent,
            });
        }
        if !in_scope(&sdom, r, self.player) {
            return Some(Snap::Leave(r));
        }
        // The client's own Player was there from the start (`first_view`):
        // its contents still arrive with the place.
        let nodes = (r == self.player || self.sim.dom().get(r).is_none()).then(|| {
            // One instance at a time, depth first.
            let mut order = Vec::new();
            let mut stack = vec![r];
            while let Some(at) = stack.pop() {
                let Some(source) = sdom.get(at) else {
                    continue;
                };
                if !in_scope(&sdom, at, self.player) || replicates_not(&sdom, at) {
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
            order
        });
        Some(Snap::Place {
            r,
            parent: sdom.parent(r),
            nodes,
        })
    }

    /// One frame's worth of what the server sent, arriving.
    fn apply(&mut self, frame: Down) {
        match frame {
            Down::Place(snaps) => self.arrive(snaps),
            Down::Ops(snaps) => {
                for snap in snaps {
                    self.deliver(snap);
                }
            }
            Down::Unreliable(packet) => self.deliver(Snap::Packet(packet)),
            Down::Physics(frame, poses) => {
                let poses = fresh(&mut self.newest_down, frame, poses);
                self.sim.place_parts(&poses);
            }
            Down::Rest {
                states,
                tracks,
                forwarded,
            } => {
                for (humanoid, state) in states {
                    self.sim.set_humanoid_state(humanoid, state);
                }
                for op in tracks {
                    self.sim.mirror_track(Some(SERVER), op);
                }
                for (humanoid, method, args) in forwarded {
                    let root = self.sim.humanoid_root(humanoid);
                    if root.is_some_and(|r| self.sim.simulates(r)) {
                        self.sim.humanoid_call(humanoid, &method, &args);
                    }
                }
            }
        }
    }

    /// The place reaches a new client: every replicated service's
    /// contents, then `game.Loaded`, then its `PlayerScripts`.
    fn arrive(&mut self, snaps: Vec<Snap>) {
        self.sim.set_remote(true);
        // The whole snapshot lands before its LocalScripts start: one in
        // PlayerGui finds ReplicatedStorage's contents.
        self.sim.hold_scripts();
        for snap in snaps {
            self.deliver(snap);
        }
        self.loaded = true;
        self.sim.set_loaded();
        self.sim.release_scripts();
        if self.sim.dom().get(self.player).is_some() {
            let db = self.sim.db();
            crate::players::player_scripts(&mut self.sim.dom_mut(), db, self.player);
            self.sim.flush();
        }
        self.sim.set_remote(false);
    }

    /// One replicated change or packet.
    fn deliver(&mut self, snap: Snap) {
        // What the server's frame moves is the server's doing; a remote's
        // handler moving a tool is the player's.
        self.sim.set_remote(!matches!(snap, Snap::Packet(_)));
        self.deliver_snap(snap);
        self.sim.set_remote(false);
    }

    fn deliver_snap(&mut self, snap: Snap) {
        match snap {
            Snap::Property {
                r,
                name,
                value,
                character,
            } => self.property(r, &name, value, character),
            Snap::Fleeting {
                r,
                class,
                name,
                parent,
            } => {
                if self.sim.dom().get(parent).is_some() {
                    if self.sim.dom().get(r).is_none() {
                        let mut dom = self.sim.dom_mut();
                        dom.insert(rbx_dom::Instance::new(r, class, name));
                        dom.set_parent(r, Some(parent));
                    }
                    self.sim.flush();
                }
            }
            Snap::Leave(r) => {
                if self.sim.dom().in_tree(r) {
                    self.sim.dom_mut().unparent(r);
                    self.sim.flush();
                }
            }
            Snap::Place { r, parent, nodes } => self.place(r, parent, nodes),
            // Destroyed: it leaves its parent (`ChildRemoved`), then goes.
            // A player leaving is `PlayerRemoving` first.
            Snap::Removed(r) => {
                let players = {
                    let dom = self.sim.dom();
                    dom.get(r)
                        .filter(|i| i.class() == "Player")
                        .and_then(|_| dom.parent(r))
                };
                if let Some(players) = players {
                    self.sim
                        .fire_signal(players, "PlayerRemoving", vec![Arg::Instance(r)]);
                }
                if self.sim.dom().in_tree(r) {
                    self.sim.dom_mut().unparent(r);
                    self.sim.flush();
                }
                if self.sim.dom().get(r).is_some() {
                    self.sim.destroy(r);
                }
            }
            Snap::Packet(packet) => self.sim.receive(None, packet),
        }
        self.characters_arrived();
    }

    fn property(
        &mut self,
        r: Ref,
        name: &str,
        value: Option<Variant>,
        character: Option<Option<Ref>>,
    ) {
        // What physics moves arrives unlogged (`place_parts`); a logged
        // move of a part this client simulates is the server's script
        // teleporting it (`PivotTo` on the player's character), which the
        // owner takes.
        let old = character.and_then(|_| {
            match self
                .sim
                .dom()
                .get(r)
                .map(|i| i.properties().get("Character"))
            {
                Some(Some(Variant::Ref(c))) => Some(*c),
                _ => None,
            }
        });
        {
            let mut dom = self.sim.dom_mut();
            let Some(target) = dom.get(r) else {
                return;
            };
            if name == "Name" {
                if let Some(Variant::String(new)) = &value {
                    if target.name() != new {
                        let _ = dom.set_name(r, new);
                    }
                }
            } else {
                match value {
                    Some(value) if target.properties().get(name) != Some(&value) => {
                        let _ = dom.set_property(r, name, value);
                    }
                    None if target.properties().contains_key(name) => {
                        let _ = dom.remove_property(r, name);
                    }
                    _ => return,
                }
            }
        }
        if let Some(new) = character {
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

    /// An instance added or moved on the server: moved if the client has
    /// it, else copied in.
    fn place(&mut self, r: Ref, parent: Option<Ref>, nodes: Option<Vec<Node>>) {
        let present = self.sim.dom().get(r).is_some();
        let own_player = r == self.player && !self.loaded;
        if present && !own_player {
            let here = self.sim.dom().parent(r);
            if here != parent && parent.is_some_and(|p| self.sim.dom().get(p).is_some()) {
                self.sim.dom_mut().set_parent(r, parent);
                self.sim.flush();
            }
            return;
        }
        if parent.is_some_and(|p| self.sim.dom().get(p).is_none()) {
            return;
        }
        let Some(order) = nodes else {
            return;
        };
        // The join's snapshot lands whole before any of its events run, so
        // a ReplicatedFirst script waiting on a module finds the module's
        // children there too. Later additions arrive one instance at a
        // time (the ReplicationOrder golden), each firing its events
        // before the next arrives.
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

    /// What the client sends the server after its frame `sent`.
    fn collect(&mut self, sent: u64, network: &Network) {
        let rtt = network.ping();
        let (unreliable, reliable): (Vec<_>, Vec<_>) = self
            .sim
            .take_outgoing()
            .into_iter()
            .filter(|o| matches!(o.to, To::Server))
            .map(|o| o.packet)
            .partition(unreliable_event);
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

        let bytes = reliable.iter().map(packet_size).sum();
        self.meters[UP_DATA].add(sent, bytes);
        self.up
            .reliable(sent, &network.up, rtt, Up::Packets(reliable));
        for packet in unreliable {
            let bytes = packet_size(&packet);
            if self
                .up
                .unreliable(sent, &network.up, Up::Unreliable(packet))
                .is_ok()
            {
                self.meters[UP_DATA].add(sent, bytes);
            }
        }
        let mut poses = std::mem::take(&mut self.carry_up);
        merge(&mut poses, parts);
        self.meters[UP_PHYSICS].add(sent, poses.len() * POSE_BYTES);
        if let Err(Up::Physics(_, lost)) =
            self.up
                .unreliable(sent, &network.up, Up::Physics(sent, poses))
        {
            self.carry_up = lost;
        }
        let rest = Up::Rest {
            states,
            tracks,
            tools,
        };
        self.up.reliable(sent, &network.up, rtt, rest);
    }
}

fn unreliable_event(packet: &Packet) -> bool {
    matches!(
        packet,
        Packet::Event {
            unreliable: true,
            ..
        }
    )
}

/// Adds `newer` poses to `poses`, each replacing an older one of its part.
fn merge(poses: &mut Vec<Placed>, newer: impl IntoIterator<Item = Placed>) {
    for placed in newer {
        match poses.iter_mut().find(|p| p.part == placed.part) {
            Some(old) => *old = placed,
            None => poses.push(placed),
        }
    }
}

/// The poses from frame `frame` that are newer than what was applied.
fn fresh(newest: &mut HashMap<Ref, u64>, frame: u64, poses: Vec<Placed>) -> Vec<Placed> {
    poses
        .into_iter()
        .filter(|placed| {
            let seen = newest.entry(placed.part).or_insert(frame);
            let keep = *seen <= frame;
            *seen = (*seen).max(frame);
            keep
        })
        .collect()
}

/// About what a pose takes on the wire (a CFrame and two velocities).
const POSE_BYTES: usize = 48;

/// About what a packet takes on the wire, for `Stats`.
fn packet_size(packet: &Packet) -> usize {
    let wires = |w: &[rbx_lua::server::Wire]| w.iter().map(|w| w.size()).sum::<usize>();
    8 + match packet {
        Packet::Event { args, .. } | Packet::Invoke { args, .. } => wires(args),
        Packet::Reply { result, .. } => match result {
            Ok(values) => wires(values),
            Err(message) => message.len(),
        },
        Packet::Chat { text, id, .. } => text.len() + id.len(),
        Packet::Drag { phase, numbers, .. } => phase.len() + 8 * numbers.len(),
    }
}

/// About what a replicated change takes on the wire, for `Stats`.
fn snap_size(snap: &Snap) -> usize {
    match snap {
        Snap::Property { name, value, .. } => {
            8 + name.len() + value.as_ref().map_or(0, crate::link::variant_size)
        }
        Snap::Fleeting { class, name, .. } => 16 + class.len() + name.len(),
        Snap::Place { nodes, .. } => {
            8 + nodes
                .iter()
                .flatten()
                .map(|(_, class, name, _, properties)| {
                    16 + class.len()
                        + name.len()
                        + properties
                            .iter()
                            .map(|(k, v)| k.len() + crate::link::variant_size(v))
                            .sum::<usize>()
                })
                .sum::<usize>()
        }
        Snap::Leave(_) | Snap::Removed(_) => 8,
        Snap::Packet(packet) => packet_size(packet),
    }
}

/// `Stats`' rates on `sim` (kilobytes a second): data received and sent,
/// physics received and sent. Engine readings: not logged, never
/// replicated.
fn write_rates(sim: &Simulation, rates: [f64; 4]) {
    let mut dom = sim.dom_mut();
    let Some(stats) = find_root(&dom, "Stats") else {
        return;
    };
    let Some(instance) = dom.get_mut(stats) else {
        return;
    };
    let properties = instance.properties_mut();
    let names = [
        "DataReceiveKbps",
        "DataSendKbps",
        "PhysicsReceiveKbps",
        "PhysicsSendKbps",
    ];
    for (name, rate) in names.into_iter().zip(rates) {
        properties.insert(name.into(), Variant::Float32(rate as f32));
    }
}

fn find_root(dom: &WeakDom, class: &str) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
}

/// The `Stats` service, made if the place has none.
fn stats_service(dom: &mut WeakDom) -> Ref {
    find_root(dom, "Stats").unwrap_or_else(|| dom.new_instance("Stats", "Stats", None))
}

/// A client's `Stats.Network.ServerStatsItem["Data Ping"]`, the round trip
/// in milliseconds that scripts read with `GetValue()`
/// ([`Simulation::set_network_ping`] keeps it current).
fn network_items(dom: &mut WeakDom) {
    let stats = stats_service(dom);
    let network = dom.new_instance("StatsItem", "Network", Some(stats));
    let server = dom.new_instance("StatsItem", "ServerStatsItem", Some(network));
    dom.new_instance("StatsItem", "Data Ping", Some(server));
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
