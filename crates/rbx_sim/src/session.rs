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
//!   running before the rest of the place arrives and `game.Loaded` fires.

use std::collections::HashSet;

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, Outgoing, Packet, To};

use crate::animation::TrackOp;
use crate::humanoid::State;
use crate::sim::{Placed, Simulation};
use crate::{Input, Output};

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
    /// What the client sends the server at the start of the next frame.
    outbox: Upstream,
    /// Parts it moved last frame, to say when they stop.
    moving: HashSet<Ref>,
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
    /// Server parts moved this frame on a client's behalf, for the others.
    relayed: Vec<Placed>,
    /// Parts the server moved last frame, to say when they stop.
    moving: HashSet<Ref>,
    fetcher: Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>,
    services: Option<rbx_lua::server::Services>,
    /// Teleport GUIs players bring into this run, by user id, placed in
    /// their client's DataModel as they join.
    arriving: std::collections::HashMap<i64, WeakDom>,
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
            relayed: Vec::new(),
            moving: HashSet::new(),
            fetcher: None,
            services: None,
            arriving: std::collections::HashMap::new(),
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
        let mut sim = Simulation::new(first_view(&self.server.dom()))?;
        sim.make_client(index, player);
        if let Some(fetcher) = &self.fetcher {
            sim.set_fetcher(fetcher.clone());
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
            outbox: Upstream::default(),
            moving: HashSet::new(),
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

        let server = &self.server;
        for client in &mut self.clients {
            let owned = server.ownership_mut().owned_by(client.player);
            client.sim.ownership_mut().owned = owned;
            if !client.loaded {
                client.arrive(server);
            } else {
                for op in &ops {
                    client.deliver(server, op);
                }
            }
            client.sim.place_parts(&moved);
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
        let client = self.clients.remove(index);
        self.server.remove_player(client.player);
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
        for client in &self.clients {
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
        for (i, client) in self.clients.iter_mut().enumerate() {
            out.push((Peer::Client(i), client.sim.take_output()));
        }
        out
    }

    pub fn set_fetcher(&mut self, fetcher: std::sync::Arc<dyn rbx_assets::AssetFetcher>) {
        self.server.set_fetcher(fetcher.clone());
        for client in &mut self.clients {
            client.sim.set_fetcher(fetcher.clone());
        }
        self.fetcher = Some(fetcher);
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
        for root in roots {
            self.deliver(server, &Op::Tree(root, None));
        }
        self.loaded = true;
        self.sim.set_loaded();
        if self.sim.dom().get(self.player).is_some() {
            let db = self.sim.db();
            crate::players::player_scripts(&mut self.sim.dom_mut(), db, self.player);
            self.sim.flush();
        }
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
        if present {
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
        drop(sdom);
        for (at, class, name, parent, properties) in order {
            if self.sim.dom().get(at).is_some() {
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
            self.sim.flush();
            if class == "Player" {
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
/// properties, and `ReplicatedFirst`'s contents.
fn first_view(server: &WeakDom) -> WeakDom {
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
            dom.remove(child);
        }
    }
    dom.take_changes();
    dom
}
