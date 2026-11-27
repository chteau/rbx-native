//! Record and replay for Local Run, to reproduce a run exactly.
//!
//! A run is a function of the place and what reaches it from outside, so a
//! recording keeps those and nothing else:
//!
//! - what was done to the session ([`Action`]: joins, each player's input,
//!   the viewport, chat, purchases, the network), by frame;
//! - what each VM read from outside ([`rbx_lua::tape`]: the clock, random
//!   seeds and draws, the web's answers and the poll each landed on) and
//!   the frame each background asset load landed;
//! - the assets and avatars fetched, by key;
//! - the place as it started, and the services' settings (the local
//!   DataStore file's contents included).
//!
//! The frame step is fixed ([`crate::FRAME_HZ`]) and the simulated link's
//! jitter and loss come from seeded generators, so neither needs keeping.
//! Each frame also keeps a hash of every peer's state (what its DOM
//! changed, by instance path, and where physics left each moving part):
//! a replay that hashes differently has diverged, and says where.

pub(crate) mod format;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rbx_dom::{Change, Ref, Variant, WeakDom};
pub use rbx_lua::tape::{Mode, Reel, Tape};

use crate::link::Network;
use crate::{Input, Peer, Session, Simulation};

pub use format::{header, FORMAT};

/// The engine version a recording names; a replay on another build may
/// diverge, and says so first.
pub const ENGINE: &str = env!("CARGO_PKG_VERSION");

/// Entry hashes a frame keeps per peer, to say what differed; a busier
/// frame keeps its total hash only.
const ENTRIES: usize = 64;

/// Something done to a running session from outside.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The server's scripts start.
    Start,
    /// A player joins, wearing `template` (a binary model) or the default
    /// character.
    Join {
        name: String,
        user_id: i64,
        template: Option<Vec<u8>>,
    },
    Leave,
    Input(Input),
    Viewport([f32; 2]),
    SafeInsets([f32; 4]),
    MenuOpen(bool),
    Chat(String),
    Reset,
    /// The test purchase dialog for the `nth` prompt the session handed
    /// out closed.
    Purchase {
        nth: usize,
        bought: bool,
    },
    Network(Network),
    /// The client fires a `RemoteEvent` (or `UnreliableRemoteEvent`) to
    /// the server with `args`, as its scripts' `FireServer` would: a load
    /// test's bots.
    Fire {
        remote: Ref,
        args: Vec<rbx_lua::server::Wire>,
    },
    /// The editor's Command Bar runs `source` in this peer's DataModel, as
    /// Studio's does in the server's or the focused client's during Play.
    Command(String),
}

/// An action as it was recorded: before frame `frame` stepped.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub frame: u64,
    pub peer: Peer,
    pub action: Action,
}

/// One peer's state after a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeerHash {
    /// Everything its DOM changed this frame, with the values it ended on.
    pub dom: u64,
    /// Where physics left each moving part.
    pub physics: u64,
    /// The change entries' own hashes, sorted (empty when `capped`).
    pub entries: Vec<u32>,
    pub capped: bool,
}

/// Every peer's state after a frame: the server's, then each client's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameHash(pub Vec<PeerHash>);

/// The services' settings as a run had them (no secrets' values, no
/// sign-in), and the local DataStore file's contents.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServicesSnapshot {
    pub universe: Option<String>,
    pub api_access: bool,
    pub place_id: i64,
    pub game_id: i64,
    pub teleport_places: Vec<i64>,
    /// touch, keyboard, mouse, gamepad.
    pub device: [bool; 4],
    pub web_lookups: bool,
    /// Live DataStores, and whether they were read-only.
    pub live: Option<bool>,
    pub assets: bool,
    /// Secret names and domains.
    pub secrets: Vec<(String, String)>,
    pub data: Option<Vec<u8>>,
}

type Blobs = Arc<Mutex<BTreeMap<String, Result<Vec<u8>, String>>>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    pub engine: String,
    pub place_hash: u64,
    /// The place as the run started (binary).
    pub place: Vec<u8>,
    pub seed: u64,
    pub services: Option<ServicesSnapshot>,
    pub actions: Vec<Logged>,
    /// After each frame, from the first.
    pub frames: Vec<FrameHash>,
    /// What each VM read: the server's, then each client's by join.
    pub reels: Vec<Reel>,
    /// Assets and avatars fetched, by key.
    pub blobs: BTreeMap<String, Result<Vec<u8>, String>>,
}

impl Recording {
    pub fn to_bytes(&self) -> Vec<u8> {
        format::encode(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Recording, String> {
        format::decode(bytes)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, self.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Recording, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Recording::from_bytes(&bytes)
    }

    pub fn frame_count(&self) -> u64 {
        self.frames.len() as u64
    }

    /// The place the run started from.
    pub fn place(&self) -> Result<WeakDom, String> {
        crate::load::parse_place(&self.place)
    }
}

/// Where recordings are kept: the user's data directory
/// (`$XDG_DATA_HOME/rbx-native/recordings`, `~/.local/share/...`,
/// `%LOCALAPPDATA%\rbx-native\recordings`, or
/// `~/Library/Application Support/rbx-native/recordings`).
pub fn default_dir() -> Option<PathBuf> {
    let env = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").or_else(|| Some(env("HOME")?.join(".local/share")))?
    };
    Some(base.join("rbx-native").join("recordings"))
}

/// A 64-bit FNV-1a hash, the same on every build and platform.
#[derive(Clone, Copy)]
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(mut self, bytes: &[u8]) -> Fnv {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
        self
    }
    fn str(self, s: &str) -> Fnv {
        self.bytes(s.as_bytes()).bytes(&[0xFF])
    }
}

/// A hash of `bytes`, for the place in a recording's header.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    Fnv::new().bytes(bytes).0
}

/// `r`'s path from the root (`Workspace.Model.Part`); the same instance
/// has the same path in a replay whatever its referent.
fn path(dom: &WeakDom, r: Ref) -> String {
    let mut names = Vec::new();
    let mut at = Some(r);
    while let Some((r, instance)) = at.and_then(|r| Some((r, dom.get(r)?))) {
        names.push(instance.name().to_owned());
        at = dom.parent(r);
    }
    names.reverse();
    names.join(".")
}

/// A value's hash; an instance it points at counts by path.
fn value(dom: &WeakDom, hash: Fnv, v: &Variant) -> Fnv {
    match v {
        Variant::Ref(r) => hash.str(&path(dom, *r)),
        other => hash.str(&format!("{other:?}")),
    }
}

/// What each traced change left, keyed by (instance path, what), with the
/// value's hash: written twice in a frame, it counts once, as it ended.
fn entries(dom: &WeakDom, changes: &[Change]) -> BTreeMap<(String, String), u64> {
    let mut out = BTreeMap::new();
    for change in changes {
        let (r, what) = match change {
            Change::Property { referent, name } => (*referent, name.clone()),
            Change::Parent { referent, .. } => (*referent, "^parent".into()),
            Change::Added(r) => (*r, "^added".into()),
            Change::Removed(r) => (*r, "^removed".into()),
            Change::Class(r) => (*r, "^class".into()),
        };
        let Some(instance) = dom.get(r) else {
            out.insert(("(gone)".to_owned(), what), 0);
            continue;
        };
        let h = match what.as_str() {
            "^parent" => Fnv::new().str(&dom.parent(r).map_or(String::new(), |p| path(dom, p))),
            "^added" | "^class" => {
                let mut props: Vec<_> = instance.properties().iter().collect();
                props.sort_by(|a, b| a.0.cmp(b.0));
                props
                    .into_iter()
                    .fold(Fnv::new().str(instance.class()), |h, (k, v)| {
                        value(dom, h.str(k), v)
                    })
            }
            name => match instance.properties().get(name) {
                Some(v) => value(dom, Fnv::new(), v),
                None => Fnv::new().str("nil"),
            },
        };
        out.insert((path(dom, r), what), h.0);
    }
    out
}

fn entry_hash(key: &(String, String), value: u64) -> u32 {
    let h = Fnv::new()
        .str(&key.0)
        .str(&key.1)
        .bytes(&value.to_le_bytes())
        .0;
    (h ^ (h >> 32)) as u32
}

/// `sim`'s state after a frame: its traced `changes` and its moving parts.
fn peer_hash(sim: &Simulation, changes: &[Change]) -> PeerHash {
    let dom = sim.dom();
    let entries = entries(&dom, changes);
    let mut total = Fnv::new();
    for (key, v) in &entries {
        total = total.str(&key.0).str(&key.1).bytes(&v.to_le_bytes());
    }
    let mut moving: Vec<(String, u64)> = sim
        .world()
        .moving_parts()
        .filter_map(|p| {
            let cframe = dom.get(p)?.properties().get("CFrame")?;
            Some((path(&dom, p), value(&dom, Fnv::new(), cframe).0))
        })
        .collect();
    moving.sort();
    let physics = moving
        .iter()
        .fold(Fnv::new(), |h, (p, v)| h.str(p).bytes(&v.to_le_bytes()));
    let capped = entries.len() > ENTRIES;
    let mut hashes: Vec<u32> = if capped {
        Vec::new()
    } else {
        entries.iter().map(|(k, v)| entry_hash(k, *v)).collect()
    };
    hashes.sort_unstable();
    PeerHash {
        dom: total.0,
        physics: physics.0,
        entries: hashes,
        capped,
    }
}

/// Where a replay first left its recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    /// The frame whose state differed (0-based).
    pub frame: u64,
    pub peer: Peer,
    pub what: String,
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let peer = match self.peer {
            Peer::Server => "the server".to_owned(),
            Peer::Client(i) => format!("client {}", i + 1),
        };
        write!(
            f,
            "diverged at frame {} ({:.2} s) on {peer}: {}",
            self.frame,
            self.frame as f64 / crate::FRAME_HZ,
            self.what
        )
    }
}

/// A session's recording or replay in progress.
pub(crate) struct Trace {
    mode: Mode,
    seed: u64,
    place: Vec<u8>,
    services: Option<ServicesSnapshot>,
    actions: Vec<Logged>,
    pub(crate) frames: Vec<FrameHash>,
    /// Each peer's changes on the last frame, to say what differed.
    pub(crate) last: Vec<Vec<Change>>,
    /// Every VM's tape, the server's first.
    tapes: Vec<Tape>,
    /// Replay: the recorded reels, for the VMs still to come.
    reels: Vec<Reel>,
    blobs: Blobs,
    /// Every purchase prompt the session handed out, in order.
    pub(crate) prompts: Vec<rbx_lua::server::PurchasePrompt>,
    /// The frame background loads are gated on, per VM.
    frame: Rc<Cell<u64>>,
}

/// A VM's `math.random` seed from the session's.
fn vm_seed(seed: u64, vm: usize) -> u32 {
    let mut z = seed.wrapping_add((vm as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) & 0x7FFF_FFFF) as u32
}

impl Trace {
    pub(crate) fn record(seed: u64, place: Vec<u8>) -> Trace {
        Trace {
            mode: Mode::Record,
            seed,
            place,
            services: None,
            actions: Vec::new(),
            frames: Vec::new(),
            last: Vec::new(),
            tapes: Vec::new(),
            reels: Vec::new(),
            blobs: Blobs::default(),
            prompts: Vec::new(),
            frame: Rc::new(Cell::new(0)),
        }
    }

    fn replay(rec: &Recording) -> Trace {
        Trace {
            mode: Mode::Replay,
            reels: rec.reels.clone(),
            blobs: Arc::new(Mutex::new(rec.blobs.clone())),
            ..Trace::record(rec.seed, Vec::new())
        }
    }

    pub(crate) fn recording(&self) -> bool {
        self.mode == Mode::Record
    }

    /// The tape for the next VM made (the server first, then each client
    /// as it joins), with the gate its background loads wait on. A
    /// recording draws its randomness from its seed: the same seed, the
    /// same run.
    pub(crate) fn next_tape(&mut self) -> (Tape, Gate) {
        let vm = self.tapes.len();
        let tape = match self.mode {
            Mode::Record => Tape::seeded(vm_seed(self.seed, vm)),
            Mode::Replay => Tape::replay(self.reels.get(vm).cloned().unwrap_or_default()),
        };
        self.tapes.push(tape.clone());
        let gate = Gate {
            tape: tape.clone(),
            frame: self.frame.clone(),
        };
        (tape, gate)
    }

    /// Keeps `action`, unless it sets a client's viewport or insets to
    /// what they already were (the editor sets them every tick).
    pub(crate) fn log(&mut self, frame: u64, peer: Peer, action: Action) {
        if !self.recording() {
            return;
        }
        if matches!(
            action,
            Action::Viewport(_) | Action::SafeInsets(_) | Action::MenuOpen(_)
        ) {
            let same = self.actions.iter().rev().find(|l| {
                l.peer == peer
                    && std::mem::discriminant(&l.action) == std::mem::discriminant(&action)
            });
            if same.is_some_and(|l| l.action == action) {
                return;
            }
        }
        {
            self.actions.push(Logged {
                frame,
                peer,
                action,
            });
        }
    }

    pub(crate) fn set_frame(&self, frame: u64) {
        self.frame.set(frame);
    }

    pub(crate) fn set_services(&mut self, config: &rbx_lua::server::Services) {
        let d = config.device;
        self.services = Some(ServicesSnapshot {
            universe: config.universe.clone(),
            api_access: config.api_access,
            place_id: config.place_id,
            game_id: config.game_id,
            teleport_places: config.teleport_places.clone(),
            device: [d.touch, d.keyboard, d.mouse, d.gamepad],
            web_lookups: config.web_lookups,
            live: config.live.as_ref().map(|l| l.read_only()),
            assets: config.assets.is_some(),
            secrets: config
                .secrets
                .iter()
                .map(|(name, (_, domain))| (name.clone(), domain.clone()))
                .collect(),
            data: config
                .data_file
                .as_ref()
                .and_then(|f| std::fs::read(f).ok()),
        });
    }

    /// The fetcher the session hands its peers: `inner`, kept from when
    /// recording, the kept answers alone when replaying.
    pub(crate) fn fetcher(
        &self,
        inner: Option<Arc<dyn rbx_assets::AssetFetcher>>,
    ) -> Arc<dyn rbx_assets::AssetFetcher> {
        Arc::new(Fetcher {
            recording: self.recording(),
            inner: inner.filter(|_| self.recording()),
            blobs: self.blobs.clone(),
        })
    }

    pub(crate) fn avatars(&self, inner: Option<Rc<dyn crate::Avatars>>) -> Rc<dyn crate::Avatars> {
        Rc::new(Avatars {
            inner: inner.filter(|_| self.recording()),
            blobs: self.blobs.clone(),
        })
    }

    pub(crate) fn finish(&self) -> Recording {
        Recording {
            engine: ENGINE.into(),
            place_hash: hash_bytes(&self.place),
            place: self.place.clone(),
            seed: self.seed,
            services: self.services.clone(),
            actions: self.actions.clone(),
            frames: self.frames.clone(),
            reels: self.tapes.iter().map(Tape::reel).collect(),
            blobs: self.blobs.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }
}

/// A background asset load's arrival, held to the frame it landed when
/// recording.
#[derive(Clone)]
pub(crate) struct Gate {
    tape: Tape,
    frame: Rc<Cell<u64>>,
}

impl Gate {
    /// The load `key` (waiting on `receiver`) if it lands now: recording,
    /// whenever it's done (and the frame is kept); replaying, on the frame
    /// it landed then, waited for if need be, never if it never landed.
    pub(crate) fn take<T>(
        gate: &Option<Gate>,
        key: impl FnOnce() -> String,
        receiver: &std::sync::mpsc::Receiver<T>,
    ) -> Option<T> {
        let Some(gate) = gate else {
            return receiver.try_recv().ok();
        };
        let frame = gate.frame.get();
        match gate.tape.mode() {
            Mode::Record => {
                let done = receiver.try_recv().ok();
                if done.is_some() {
                    gate.tape.landed(key(), frame);
                }
                done
            }
            Mode::Replay => match gate.tape.due(&key()) {
                Some(due) if due <= frame => receiver.recv().ok(),
                _ => None,
            },
        }
    }
}

fn blob(blobs: &Blobs, key: &str) -> Option<Result<Vec<u8>, String>> {
    blobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
}

/// Keeps `value` for `key`, unless one was kept already: then that one,
/// which every later load of `key` gets too. Peers load the same assets on
/// their own threads, and a replay serves them all the one kept answer, so
/// a recording must as well, or a cache or network hiccup between two
/// peers' loads replays differently.
fn keep(
    blobs: &Blobs,
    key: String,
    value: Result<Vec<u8>, String>,
) -> Option<Result<Vec<u8>, String>> {
    match blobs.lock().unwrap_or_else(|e| e.into_inner()).entry(key) {
        std::collections::btree_map::Entry::Occupied(kept) => Some(kept.get().clone()),
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(value);
            None
        }
    }
}

/// The asset cache's copy of `id`, unless `fetcher` reads the cache itself
/// (a recording or replay's, which keeps cache hits too).
pub(crate) fn cached(
    fetcher: Option<&Arc<dyn rbx_assets::AssetFetcher>>,
    id: u64,
) -> Option<Vec<u8>> {
    if fetcher.is_some_and(|f| f.caches()) {
        return None;
    }
    rbx_assets::AssetCache::new(None).ok()?.get_id(id)
}

/// Assets by id: from the asset cache or fetched, and kept; or the kept
/// ones.
struct Fetcher {
    recording: bool,
    inner: Option<Arc<dyn rbx_assets::AssetFetcher>>,
    blobs: Blobs,
}

impl Fetcher {
    fn get(
        &self,
        key: String,
        id: u64,
        fetch: impl FnOnce(&dyn rbx_assets::AssetFetcher) -> Result<Vec<u8>, rbx_assets::FetchError>,
    ) -> Result<Vec<u8>, rbx_assets::FetchError> {
        let refused = |message| rbx_assets::FetchError::Refused { id, message };
        if self.recording {
            if let Some(kept) = blob(&self.blobs, &key) {
                return kept.map_err(refused);
            }
            let cached = key
                .starts_with("asset:")
                .then(|| rbx_assets::AssetCache::new(None).ok()?.get_id(id))
                .flatten();
            let got = match (cached, &self.inner) {
                (Some(bytes), _) => Ok(bytes),
                (None, Some(inner)) => fetch(inner.as_ref()),
                (None, None) => Err(rbx_assets::FetchError::NotFound(id)),
            };
            let value = got.as_ref().map(|b| b.clone()).map_err(|e| e.to_string());
            return match keep(&self.blobs, key, value) {
                Some(kept) => kept.map_err(refused),
                None => got,
            };
        }
        match &self.inner {
            Some(inner) => {
                let got = fetch(inner.as_ref());
                let value = got.as_ref().map(|b| b.clone()).map_err(|e| e.to_string());
                match keep(&self.blobs, key, value) {
                    Some(kept) => kept.map_err(refused),
                    None => got,
                }
            }
            None => match blob(&self.blobs, &key) {
                Some(kept) => kept.map_err(refused),
                None => Err(rbx_assets::FetchError::NotFound(id)),
            },
        }
    }
}

impl rbx_assets::AssetFetcher for Fetcher {
    fn caches(&self) -> bool {
        true
    }

    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.get(format!("asset:{id}"), id, |f| f.fetch_id(id))
    }

    fn fetch_thumb(&self, spec: &str) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.get(format!("thumb:{spec}"), 0, |f| f.fetch_thumb(spec))
    }
}

/// Avatars: looked up and kept (as binary models), or the kept ones.
struct Avatars {
    inner: Option<Rc<dyn crate::Avatars>>,
    blobs: Blobs,
}

impl Avatars {
    fn get(
        &self,
        key: String,
        look: impl FnOnce(&dyn crate::Avatars) -> Result<WeakDom, String>,
    ) -> Result<WeakDom, String> {
        let kept = match (&self.inner, blob(&self.blobs, &key)) {
            (_, Some(kept)) => kept,
            (Some(inner), None) => {
                let got = look(inner.as_ref());
                let value = match &got {
                    Ok(dom) => rbx_binary::serialize(dom).map_err(|e| e.to_string()),
                    Err(e) => Err(e.clone()),
                };
                match keep(&self.blobs, key, value) {
                    Some(kept) => kept,
                    None => return got,
                }
            }
            (None, None) => Err(format!("{key} is not in the recording")),
        }?;
        rbx_binary::deserialize(&kept).map_err(|e| e.to_string())
    }
}

impl crate::Avatars for Avatars {
    fn description(&self, user_id: i64) -> Result<WeakDom, String> {
        self.get(format!("description:{user_id}"), |a| a.description(user_id))
    }

    fn character(&self, description: &WeakDom) -> Result<WeakDom, String> {
        let bytes = rbx_binary::serialize(description).unwrap_or_default();
        self.get(format!("character:{:016x}", hash_bytes(&bytes)), |a| {
            a.character(description)
        })
    }

    fn outfit(&self, outfit_id: i64) -> Result<WeakDom, String> {
        self.get(format!("outfit:{outfit_id}"), |a| a.outfit(outfit_id))
    }
}

/// Answers nothing: a replay's live DataStores, whose answers are on the
/// tape.
#[derive(Debug)]
struct Replayed {
    read_only: bool,
}

impl rbx_lua::server::LiveData for Replayed {
    fn call(&self, op: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, String> {
        Err(format!("{op} is not in the recording"))
    }

    fn read_only(&self) -> bool {
        self.read_only
    }
}

#[derive(Debug)]
struct NoAssets;

impl rbx_lua::server::AssetSource for NoAssets {
    fn fetch(&self, id: u64) -> Result<Vec<u8>, String> {
        Err(format!("asset {id} is not in the recording"))
    }
}

/// Services as `snapshot` had them; the DataStore file is a copy at
/// `data_file`, so a replay never touches the real one.
fn services(snapshot: &ServicesSnapshot, data_file: &Path) -> rbx_lua::server::Services {
    let [touch, keyboard, mouse, gamepad] = snapshot.device;
    let data_file = snapshot.data.as_ref().and_then(|data| {
        std::fs::write(data_file, data)
            .ok()
            .map(|_| data_file.to_owned())
    });
    rbx_lua::server::Services {
        universe: snapshot.universe.clone(),
        data_file,
        api_access: snapshot.api_access,
        place_id: snapshot.place_id,
        game_id: snapshot.game_id,
        teleport_places: snapshot.teleport_places.clone(),
        device: rbx_lua::server::Device {
            touch,
            keyboard,
            mouse,
            gamepad,
        },
        web_lookups: snapshot.web_lookups,
        live: snapshot
            .live
            .map(|read_only| Arc::new(Replayed { read_only }) as _),
        assets: snapshot.assets.then(|| Arc::new(NoAssets) as _),
        secrets: snapshot
            .secrets
            .iter()
            .map(|(name, domain)| (name.clone(), (String::new(), domain.clone())))
            .collect(),
        ..rbx_lua::server::Services::default()
    }
}

/// A recording played back into a session it made: the same place, the
/// same actions on the same frames, every VM reading its tape.
pub struct Playback {
    recording: Recording,
    next: usize,
    /// The DataStore file's copy, removed with the playback.
    data_file: PathBuf,
}

impl Playback {
    /// The session to play `recording` back into, not started (its first
    /// recorded action starts it), and the playback.
    pub fn start(recording: Recording) -> Result<(Session, Playback), String> {
        static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let place = recording.place()?;
        let mut session = Session::new(place)?;
        session.replay_trace(Trace::replay(&recording));
        let data_file = std::env::temp_dir().join(format!(
            "rbxnative-replay-{}-{}.json",
            std::process::id(),
            RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        if let Some(snapshot) = &recording.services {
            session.set_services(services(snapshot, &data_file));
        }
        let playback = Playback {
            recording,
            next: 0,
            data_file,
        };
        Ok((session, playback))
    }

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn frame_count(&self) -> u64 {
        self.recording.frame_count()
    }

    /// One frame of `session`: the actions recorded before it, the step,
    /// then its state against the recording's. Purchase prompts are taken
    /// here (the recorded answers close them).
    pub fn step(&mut self, session: &mut Session) -> Result<(), Divergence> {
        let frame = session.server.frame();
        while let Some(logged) = self.recording.actions.get(self.next) {
            if logged.frame > frame {
                break;
            }
            let (peer, action) = (logged.peer, logged.action.clone());
            self.next += 1;
            session.act(peer, action);
        }
        session.step();
        session.take_prompts();
        let (Some(trace), Some(want)) = (&session.trace, self.recording.frames.get(frame as usize))
        else {
            return Ok(());
        };
        let Some(got) = trace.frames.last() else {
            return Ok(());
        };
        compare(frame, got, want, |i, recorded| {
            let peer = if i == 0 {
                Peer::Server
            } else {
                Peer::Client(i - 1)
            };
            match (session.sim(peer), trace.last.get(i)) {
                (Some(sim), Some(changes)) => describe(&sim.dom(), changes, recorded),
                _ => String::new(),
            }
        })
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.data_file);
    }
}

/// A [`Playback`] with its session, keeping what was printed: for headless
/// replays and tests.
pub struct Replay {
    pub session: Session,
    pub playback: Playback,
    /// Lines printed so far, with the frame and the peer.
    pub output: Vec<(u64, Peer, rbx_lua::Output)>,
}

impl Replay {
    pub fn new(recording: Recording) -> Result<Replay, String> {
        let (session, playback) = Playback::start(recording)?;
        Ok(Replay {
            session,
            playback,
            output: Vec::new(),
        })
    }

    /// The frame about to step.
    pub fn frame(&self) -> u64 {
        self.session.server.frame()
    }

    pub fn step(&mut self) -> Result<(), Divergence> {
        let frame = self.frame();
        let result = self.playback.step(&mut self.session);
        for (peer, output) in self.session.take_output() {
            if !output.lines().is_empty() {
                self.output.push((frame, peer, output));
            }
        }
        result
    }

    /// Steps to frame `frame` (or the end), stopping at a divergence.
    pub fn run_to(&mut self, frame: u64) -> Result<(), Divergence> {
        let end = frame.min(self.playback.frame_count());
        while self.frame() < end {
            self.step()?;
        }
        Ok(())
    }

    /// Script errors printed so far: (frame, peer, text).
    pub fn errors(&self) -> Vec<(u64, Peer, String)> {
        self.output
            .iter()
            .flat_map(|(frame, peer, output)| {
                output
                    .lines()
                    .iter()
                    .zip(output.kinds())
                    .filter(|(_, kind)| **kind == rbx_lua::LineKind::Error)
                    .map(|(text, _)| (*frame, *peer, text.clone()))
            })
            .collect()
    }
}

/// The first difference between a frame's state and the recorded one;
/// `describe(i, entries)` lists peer `i`'s changes the recording lacks.
fn compare(
    frame: u64,
    got: &FrameHash,
    want: &FrameHash,
    describe: impl Fn(usize, &[u32]) -> String,
) -> Result<(), Divergence> {
    let peer_of = |i: usize| match i {
        0 => Peer::Server,
        i => Peer::Client(i - 1),
    };
    if got.0.len() != want.0.len() {
        return Err(Divergence {
            frame,
            peer: Peer::Server,
            what: format!(
                "{} peers in the replay, {} recorded",
                got.0.len(),
                want.0.len()
            ),
        });
    }
    for (i, (g, w)) in got.0.iter().zip(&want.0).enumerate() {
        let what = if g.dom != w.dom {
            let counts = if g.capped || w.capped {
                String::new()
            } else {
                let only =
                    |a: &[u32], b: &[u32]| a.iter().filter(|e| b.binary_search(e).is_err()).count();
                format!(
                    " ({} change(s) only in the replay, {} only in the recording)",
                    only(&g.entries, &w.entries),
                    only(&w.entries, &g.entries)
                )
            };
            format!(
                "the DOM changed differently{counts}{}",
                describe(i, &w.entries)
            )
        } else if g.physics != w.physics {
            "physics left the moving parts elsewhere".to_owned()
        } else {
            continue;
        };
        return Err(Divergence {
            frame,
            peer: peer_of(i),
            what,
        });
    }
    Ok(())
}

/// `changes` (a frame's, on `dom`) that `recorded` (entry hashes) lacks,
/// a few of them as `path.property = value`.
fn describe(dom: &WeakDom, changes: &[Change], recorded: &[u32]) -> String {
    let mut lines = Vec::new();
    for (key, h) in entries(dom, changes) {
        if recorded.binary_search(&entry_hash(&key, h)).is_ok() {
            continue;
        }
        let (at, what) = key;
        let shown = find(dom, &at)
            .and_then(|r| dom.get(r))
            .and_then(|i| i.properties().get(&what))
            .map(|v| format!(" = {v:?}"))
            .unwrap_or_default();
        lines.push(format!("{at}.{what}{shown}"));
        if lines.len() == 5 {
            break;
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("; in the replay: {}", lines.join(", "))
    }
}

/// The instance at `path`, if one is.
fn find(dom: &WeakDom, path: &str) -> Option<Ref> {
    let mut names = path.split('.');
    let first = names.next()?;
    let named = |r: &Ref, name: &str| dom.get(*r).is_some_and(|i| i.name() == name);
    let mut at = *dom.root_refs().iter().find(|r| named(r, first))?;
    for name in names {
        at = *dom.get(at)?.children().iter().find(|c| named(c, name))?;
    }
    Some(at)
}

/// Every peer's state after a frame (the server's, then each client's),
/// with the changes it was hashed from.
pub(crate) fn hash_frame(
    server: &mut Simulation,
    clients: &mut [crate::session::Client],
) -> (FrameHash, Vec<Vec<Change>>) {
    let (hashes, changes) = std::iter::once(server)
        .chain(clients.iter_mut().map(|c| &mut c.sim))
        .map(|sim| {
            let changes = sim.take_trace();
            (peer_hash(sim, &changes), changes)
        })
        .unzip();
    (FrameHash(hashes), changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Answers once, then fails: a network or cache hiccup between two
    /// peers' loads of one asset.
    struct Flaky(AtomicU32);

    impl rbx_assets::AssetFetcher for Flaky {
        fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
            match self.0.fetch_add(1, Ordering::Relaxed) {
                0 => Ok(vec![1, 2, 3]),
                _ => Err(rbx_assets::FetchError::NotFound(id)),
            }
        }
    }

    #[test]
    fn every_load_of_an_asset_gets_the_answer_the_recording_keeps() {
        let trace = Trace::record(1, Vec::new());
        let id = u64::MAX - 7;
        let server = trace.fetcher(Some(Arc::new(Flaky(AtomicU32::new(0)))));
        let client = trace.fetcher(Some(Arc::new(Flaky(AtomicU32::new(1)))));
        assert_eq!(server.fetch_id(id).unwrap(), [1, 2, 3]);
        assert_eq!(client.fetch_id(id).unwrap(), [1, 2, 3]);
        assert_eq!(
            trace.finish().blobs[&format!("asset:{id}")],
            Ok(vec![1, 2, 3])
        );
    }
}
