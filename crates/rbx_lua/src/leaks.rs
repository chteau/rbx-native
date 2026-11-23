//! The leak finder's figures for one VM (a Local Run peer): what keeps
//! growing while a game runs. Nothing is tracked, and nothing costs, until
//! the editor's Leak finder dock calls [`keep_alive`] (it does every frame
//! it is open); a second without one and tracking stops again. While on,
//! a [`Sample`] is taken every [`SAMPLE_EVERY`] seconds of game time:
//!
//! - connections (Roblox's `Signals` memory tag): how many each script
//!   made, and the ones still connected to an instance that left the
//!   place (parented to nil) or was removed;
//! - instances (`Instances`): nil-parented ones and destroyed ones a
//!   script still holds, by class and by the `Instance.new`/`Clone` call
//!   that made them (call sites are recorded while tracking);
//! - tables: the largest ones reachable from each script's environment,
//!   its functions' upvalues, its threads' locals, its connection handlers
//!   and module results, followed by identity from sample to sample;
//! - the Luau heap (`LuaHeap`), split per script through Luau's memory
//!   categories, named as `debug.setmemorycategory` names them: the
//!   script's own, unless it set one.
//!
//! [`suspects`] reads the history for what keeps growing, [`diff`] compares
//! two samples.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_int;
use std::time::{Duration, Instant};

use mlua::{ffi, Function, Lua, MultiValue, Table, Thread, Value};
use rbx_dom::Ref;

use crate::engine::scheduler::{internal_chunk, script_frames};
use crate::engine::signal::Owner;
use crate::engine::Engine;

/// Game seconds between two samples.
pub const SAMPLE_EVERY: f64 = 2.0;
/// Samples kept: ten minutes.
pub const KEPT: usize = 300;
/// How many samples in a row a figure must have risen over to be a
/// suspect: a sawtooth (a cache filled and cleared) never is.
pub const GROWING: usize = 4;
/// Without a [`keep_alive`] for this long, tracking stops.
const KEEP_ALIVE: Duration = Duration::from_secs(1);
/// Tables, functions and threads one walk visits at most.
const WALK_NODES: usize = 100_000;
/// Table entries one walk reads at most.
const WALK_ENTRIES: usize = 1_000_000;
/// The largest tables a sample keeps (besides the ones it follows).
const TOP_TABLES: usize = 30;
/// Luau has 256 memory categories; 0 is everything not a script's.
const CATEGORIES: usize = 255;

/// Where something was made: a script's full name and a line (0 when only
/// the script is known).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Site {
    pub script: String,
    pub line: usize,
}

impl Site {
    /// `Leaky:12`, as Studio prints a location.
    pub fn short(&self) -> String {
        let name = self.script.rsplit('.').next().unwrap_or(&self.script);
        match self.line {
            0 => name.to_owned(),
            line => format!("{name}:{line}"),
        }
    }
}

/// One script's (or memory category's) share.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptStat {
    /// The script's full name, or the category `debug.setmemorycategory`
    /// set.
    pub name: String,
    /// Live Luau heap allocated under it while tracking.
    pub heap: usize,
    pub connections: usize,
}

/// Connections from one site to one event whose owner left the place.
#[derive(Clone, Debug, PartialEq)]
pub struct StaleConnections {
    pub site: Site,
    pub event: String,
    /// The owner's class.
    pub owner: String,
    /// Removed (destroyed, or gone with replication) rather than parented
    /// to nil.
    pub destroyed: bool,
    pub count: usize,
    /// Samples in a row the oldest of them has been stale.
    pub streak: u32,
}

/// Instances out of the place, of one class from one site.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceGroup {
    pub class: String,
    /// The `Instance.new`/`Clone` call, when it ran while tracking.
    pub site: Option<Site>,
    /// Destroyed and still held, rather than parented to nil.
    pub destroyed: bool,
    /// Held by a script (its handle not collected): what Roblox keeps.
    pub held: usize,
    /// Nil-parented and held by nothing: Roblox would collect these.
    pub unheld: usize,
}

/// A table reachable from a script.
#[derive(Clone, Debug, PartialEq)]
pub struct TableStat {
    /// Its address: the same table from sample to sample.
    pub id: usize,
    /// The script (and line) it was first reached from.
    pub site: Site,
    /// How it was reached: `local cache`, `env.players`, `handler↑seen`…
    pub path: String,
    pub entries: usize,
}

/// One look at a VM.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// Game time, seconds.
    pub time: f64,
    /// The whole Luau heap.
    pub heap: usize,
    /// Heap not under any script's category (the engine's, and whatever
    /// was allocated before tracking started).
    pub unattributed: usize,
    pub scripts: Vec<ScriptStat>,
    pub connections: usize,
    pub stale: Vec<StaleConnections>,
    pub instances: Vec<InstanceGroup>,
    pub tables: Vec<TableStat>,
    /// The table walk hit its budget: some tables went unseen.
    pub truncated: bool,
}

impl Sample {
    /// Instances out of the place that a script holds.
    pub fn held_instances(&self) -> usize {
        self.instances.iter().map(|g| g.held).sum()
    }

    /// Entries in the tables this sample kept.
    pub fn table_entries(&self) -> usize {
        self.tables.iter().map(|t| t.entries).sum()
    }
}

/// What a suspect is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Signals,
    Instances,
    Tables,
}

impl Kind {
    /// The Developer Console's memory tag where there is one.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Signals => "Signals",
            Kind::Instances => "Instances",
            Kind::Tables => "Luau tables",
        }
    }
}

/// Something that looks like a leak.
#[derive(Clone, Debug, PartialEq)]
pub struct Suspect {
    pub kind: Kind,
    pub what: String,
    pub site: Option<Site>,
    pub count: usize,
    /// How much it rose over the last [`GROWING`] samples (0 for a stale
    /// connection, which is a suspect for what it is).
    pub growth: usize,
}

/// A VM's tracking state, in its app data.
#[derive(Default)]
pub struct Tracker {
    alive: Option<Instant>,
    /// Who made each instance made while tracking, and its class.
    sites: HashMap<Ref, (Site, String)>,
    /// Memory category `i + 1`'s name.
    categories: Vec<String>,
    next: f64,
    /// Connection id → samples in a row it has been stale.
    streaks: HashMap<u32, u32>,
    history: VecDeque<Sample>,
}

impl Tracker {
    pub fn on(&self) -> bool {
        self.alive.is_some_and(|t| t.elapsed() < KEEP_ALIVE)
    }

    /// Every sample kept, oldest first.
    pub fn history(&self) -> &VecDeque<Sample> {
        &self.history
    }

    /// The memory category for `name`, 0 once all are taken.
    fn category(&mut self, name: &str) -> u8 {
        if let Some(i) = self.categories.iter().position(|c| c == name) {
            return i as u8 + 1;
        }
        if self.categories.len() == CATEGORIES {
            return 0;
        }
        self.categories.push(name.to_owned());
        self.categories.len() as u8
    }
}

#[derive(Clone)]
struct Shared(std::rc::Rc<std::cell::RefCell<Tracker>>);

/// The tracker of `lua`, if the dock ever asked for one.
pub(crate) fn tracker(lua: &Lua) -> Option<std::rc::Rc<std::cell::RefCell<Tracker>>> {
    lua.app_data_ref::<Shared>().map(|s| s.0.clone())
}

fn tracking(lua: &Lua) -> Option<std::rc::Rc<std::cell::RefCell<Tracker>>> {
    tracker(lua).filter(|t| t.borrow().on())
}

/// Keeps tracking on for another second. Turning it on tags each script's
/// main thread with the script's memory category.
pub(crate) fn keep_alive(lua: &Lua) {
    let tracker = match tracker(lua) {
        Some(tracker) => tracker,
        None => {
            let tracker = std::rc::Rc::new(std::cell::RefCell::new(Tracker::default()));
            lua.set_app_data(Shared(tracker.clone()));
            tracker
        }
    };
    let was = tracker.borrow().on();
    tracker.borrow_mut().alive = Some(Instant::now());
    let Some(engine) = lua.app_data_ref::<std::rc::Rc<Engine>>().map(|e| e.clone()) else {
        return;
    };
    if was {
        return;
    }
    let owners = engine.scheduler.borrow().owners.clone();
    for (thread, script) in owners {
        let name = crate::instance::tree::full_name(&engine.ctx.dom(), script);
        if let Some(name) = name {
            let id = tracker.borrow_mut().category(&name);
            set_category(lua, &thread, id);
        }
    }
}

/// After a step: a sample when one is due.
pub(crate) fn tick(lua: &Lua, now: f64) {
    let Some(tracker) = tracking(lua) else {
        return;
    };
    if now < tracker.borrow().next {
        return;
    }
    tracker.borrow_mut().next = now + SAMPLE_EVERY;
    let sample = sample(lua, now);
    let mut tracker = tracker.borrow_mut();
    if tracker.history.len() == KEPT {
        tracker.history.pop_front();
    }
    tracker.history.push_back(sample);
}

/// `Instance.new` or `Clone` made `referent`: notes the script line that
/// asked, while tracking.
pub(crate) fn created(lua: &Lua, referent: Ref, class: &str) {
    let Some(tracker) = tracking(lua) else {
        return;
    };
    let site = script_frames(lua)
        .into_iter()
        .next()
        .map(|(script, line, _)| Site {
            script,
            line: line.unwrap_or(0),
        })
        .unwrap_or_default();
    tracker
        .borrow_mut()
        .sites
        .insert(referent, (site, class.to_owned()));
}

/// A new scheduler thread about to run `func`: its allocations go to the
/// category of the script `func` is in, while tracking.
pub(crate) fn thread_started(lua: &Lua, thread: &Thread, func: &Function) {
    let Some(tracker) = tracking(lua) else {
        return;
    };
    let Some(site) = site_of(func) else {
        return;
    };
    let id = tracker.borrow_mut().category(&site.script);
    set_category(lua, thread, id);
}

/// `debug.setmemorycategory(name)` (`None`: `resetmemorycategory`, back to
/// the running script's own) for the running thread, while tracking.
pub(crate) fn set_memory_category(lua: &Lua, name: Option<&str>) {
    let Some(tracker) = tracking(lua) else {
        return;
    };
    let name = match name {
        Some(name) => name.to_owned(),
        None => match script_frames(lua).pop() {
            Some((script, _, _)) => script,
            None => return,
        },
    };
    let id = tracker.borrow_mut().category(&name);
    set_category(lua, &lua.current_thread(), id);
}

fn set_category(lua: &Lua, thread: &Thread, id: u8) {
    // SAFETY: reads the argument thread off the stack and sets its
    // allocation category, which is always below LUA_MEMORY_CATEGORIES.
    let _ = unsafe {
        lua.exec_raw::<()>(thread.clone(), |state| {
            let co = ffi::lua_tothread(state, 1);
            if !co.is_null() {
                ffi::lua_setmemcat(co, c_int::from(id));
            }
            ffi::lua_settop(state, 0);
        })
    };
}

/// Where `func` was written, unless it is the engine's or a native one.
fn site_of(func: &Function) -> Option<Site> {
    let info = func.info();
    let source = info.source?;
    let script = source.strip_prefix('=')?;
    if internal_chunk(script) {
        return None;
    }
    Some(Site {
        script: script.to_owned(),
        line: info.line_defined.unwrap_or(0),
    })
}

/// Takes a sample now (the dock's Take snapshot), tracking or not.
pub(crate) fn sample(lua: &Lua, now: f64) -> Sample {
    let mut out = Sample {
        time: now,
        heap: lua.used_memory(),
        ..Sample::default()
    };
    let Some(engine) = lua.app_data_ref::<std::rc::Rc<Engine>>().map(|e| e.clone()) else {
        return out;
    };
    let shared = tracker(lua);
    let mut by_script: HashMap<String, ScriptStat> = HashMap::new();

    if let Some(tracker) = &shared {
        let names = tracker.borrow().categories.clone();
        let mut bytes = vec![0usize; names.len() + 1];
        // SAFETY: only reads the per-category byte counts.
        let _ = unsafe {
            lua.exec_raw::<()>((), |state| {
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = ffi::lua_totalbytes(state, i as c_int);
                }
            })
        };
        out.unattributed = bytes[0];
        for (name, heap) in names.into_iter().zip(bytes.into_iter().skip(1)) {
            by_script.insert(
                name.clone(),
                ScriptStat {
                    name,
                    heap,
                    connections: 0,
                },
            );
        }
    }

    let ctx = engine.ctx.clone();
    let dom = ctx.dom();
    let mut handlers = Vec::new();
    {
        let signals = engine.signals.borrow();
        let tracker = shared.as_ref().map(|t| t.borrow());
        let class_of = |r: Ref| {
            dom.get(r)
                .map(|i| i.class().to_string())
                .or_else(|| ctx.tombstone(r).map(|t| t.0))
                .or_else(|| tracker.as_ref()?.sites.get(&r).map(|s| s.1.clone()))
                .unwrap_or_else(|| "Instance".to_owned())
        };
        let mut stale: HashMap<(Site, String, String, bool), (usize, u32)> = HashMap::new();
        let mut streaks = HashMap::new();
        let old = shared
            .as_ref()
            .map(|t| t.borrow().streaks.clone())
            .unwrap_or_default();
        for (key, list) in &signals.connections {
            for connection in list {
                out.connections += 1;
                let site = site_of(&connection.func);
                if let Some(site) = &site {
                    by_script
                        .entry(site.script.clone())
                        .or_insert_with(|| ScriptStat {
                            name: site.script.clone(),
                            ..ScriptStat::default()
                        })
                        .connections += 1;
                }
                handlers.push((key.event.clone(), connection.func.clone()));
                let Owner::Instance(owner) = key.owner else {
                    continue;
                };
                let destroyed = dom.get(owner).is_none();
                if !destroyed && dom.in_tree(owner) {
                    continue;
                }
                let streak = old.get(&connection.id).copied().unwrap_or(0) + 1;
                streaks.insert(connection.id, streak);
                let event = key.event.split(':').next().unwrap_or(&key.event).to_owned();
                let entry = stale
                    .entry((site.unwrap_or_default(), event, class_of(owner), destroyed))
                    .or_default();
                entry.0 += 1;
                entry.1 = entry.1.max(streak);
            }
        }
        drop(tracker);
        if let Some(tracker) = &shared {
            tracker.borrow_mut().streaks = streaks;
        }
        out.stale = stale
            .into_iter()
            .map(
                |((site, event, owner, destroyed), (count, streak))| StaleConnections {
                    site,
                    event,
                    owner,
                    destroyed,
                    count,
                    streak,
                },
            )
            .collect();
        out.stale
            .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.site.cmp(&b.site)));
    }

    let followed: HashSet<(usize, String)> = shared
        .as_ref()
        .and_then(|t| {
            t.borrow()
                .history
                .back()
                .map(|s| s.tables.iter().map(|t| (t.id, t.path.clone())).collect())
        })
        .unwrap_or_default();
    let walked = walk(lua, &engine, handlers);
    out.truncated = walked.truncated;
    out.tables = keep_tables(walked.tables, &followed);

    // Instances out of the place: nil-parented trees, held when a script
    // reaches the top or anything under it, and destroyed ones still
    // reached. A whole walk tells exactly; a cut-short one falls back on
    // the handles not collected yet, which garbage keeps for a while.
    {
        let remains = ctx.remains();
        let tracker = shared.as_ref().map(|t| t.borrow());
        let site_of_ref = |r: Ref| tracker.as_ref().and_then(|t| t.sites.get(&r).cloned());
        let reached: Box<dyn Fn(&Ref) -> bool> = match walked.truncated {
            false => Box::new(|r| walked.instances.contains(r)),
            true => Box::new(|_| true),
        };
        let mut held_tops = HashSet::new();
        for &r in remains.held().filter(|r| reached(r)) {
            let mut top = r;
            while let Some(parent) = dom.parent(top) {
                top = parent;
            }
            held_tops.insert(top);
        }
        let mut groups: HashMap<(String, Option<Site>, bool), (usize, usize)> = HashMap::new();
        for top in dom.unparented_refs() {
            let Some(instance) = dom.get(top) else {
                continue;
            };
            let site = site_of_ref(top);
            let class = instance.class().to_string();
            let entry = groups.entry((class, site.map(|s| s.0), false)).or_default();
            match held_tops.contains(&top) {
                true => entry.0 += 1,
                false => entry.1 += 1,
            }
        }
        for (r, instance) in remains.kept().filter(|(r, _)| reached(r)) {
            let site = site_of_ref(*r);
            groups
                .entry((instance.class().to_string(), site.map(|s| s.0), true))
                .or_default()
                .0 += 1;
        }
        out.instances = groups
            .into_iter()
            .map(|((class, site, destroyed), (held, unheld))| InstanceGroup {
                class,
                site,
                destroyed,
                held,
                unheld,
            })
            .collect();
        out.instances.sort_by(|a, b| {
            b.held
                .cmp(&a.held)
                .then_with(|| b.unheld.cmp(&a.unheld))
                .then_with(|| a.class.cmp(&b.class))
        });
    }
    drop(dom);

    // Instances made while tracking that are gone and held by nothing need
    // no call site any more.
    if let Some(tracker) = &shared {
        let dom = ctx.dom();
        let remains = ctx.remains();
        tracker
            .borrow_mut()
            .sites
            .retain(|r, _| dom.get(*r).is_some() || remains.get(*r).is_some());
    }

    out.scripts = by_script.into_values().collect();
    out.scripts.sort_by(|a, b| {
        b.heap
            .cmp(&a.heap)
            .then_with(|| b.connections.cmp(&a.connections))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// The [`TOP_TABLES`] largest, and every one the last sample kept.
fn keep_tables(mut tables: Vec<TableStat>, followed: &HashSet<(usize, String)>) -> Vec<TableStat> {
    tables.sort_by(|a, b| b.entries.cmp(&a.entries).then_with(|| a.path.cmp(&b.path)));
    let mut kept = 0;
    tables.retain(|t| {
        let top = kept < TOP_TABLES && t.entries > 0;
        if top {
            kept += 1;
        }
        top || followed.contains(&(t.id, t.path.clone()))
    });
    tables
}

/// Breadth first from every script's roots, each table counted once, for
/// the first script to reach it.
struct Walk<'a> {
    lua: &'a Lua,
    seen: HashSet<usize>,
    queue: VecDeque<(Value, u32, String)>,
    origins: Vec<Site>,
    tables: Vec<TableStat>,
    /// Instances whose handle was reached.
    instances: HashSet<Ref>,
    nodes: usize,
    entries: usize,
    truncated: bool,
}

const PATH_MAX: usize = 96;

fn child_path(path: &str, key: &Value) -> String {
    let mut out = match key {
        Value::String(s) => {
            let s = s.to_string_lossy();
            let ident = s
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && s.chars().all(|c| c.is_alphanumeric() || c == '_');
            match ident {
                true => format!("{path}.{s}"),
                false => format!("{path}[{s:?}]"),
            }
        }
        Value::Integer(i) => format!("{path}[{i}]"),
        Value::Number(n) => format!("{path}[{n}]"),
        other => format!("{path}[{}]", other.type_name()),
    };
    if out.len() > PATH_MAX {
        let mut cut = PATH_MAX;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push('…');
    }
    out
}

impl Walk<'_> {
    fn origin(&mut self, site: Site) -> u32 {
        match self.origins.iter().position(|s| *s == site) {
            Some(i) => i as u32,
            None => {
                self.origins.push(site);
                self.origins.len() as u32 - 1
            }
        }
    }

    fn push(&mut self, value: Value, origin: u32, path: String) {
        if let Value::UserData(data) = &value {
            if let Ok(instance) = data.borrow::<crate::instance::LuaInstance>() {
                self.instances.insert(instance.referent());
            }
            return;
        }
        if !matches!(
            value,
            Value::Table(_) | Value::Function(_) | Value::Thread(_)
        ) {
            return;
        }
        if self.seen.insert(value.to_pointer() as usize) {
            self.queue.push_back((value, origin, path));
        }
    }

    fn run(&mut self) {
        while let Some((value, origin, path)) = self.queue.pop_front() {
            if self.nodes == 0 || self.entries == 0 {
                self.truncated = true;
                return;
            }
            self.nodes -= 1;
            match value {
                Value::Table(table) => self.table(table, origin, path),
                Value::Function(func) => self.function(func, origin, path),
                Value::Thread(thread) => self.thread(&thread, Some(origin)),
                _ => {}
            }
        }
    }

    fn table(&mut self, table: Table, origin: u32, path: String) {
        let id = table.to_pointer() as usize;
        let mut entries = 0;
        let mut children = Vec::new();
        for pair in table.clone().pairs::<Value, Value>() {
            let Ok((key, value)) = pair else {
                continue;
            };
            if self.entries == 0 {
                self.truncated = true;
                return;
            }
            self.entries -= 1;
            entries += 1;
            if walkable(&value) {
                children.push((child_path(&path, &key), value));
            }
            if walkable(&key) {
                children.push((format!("{path}[key]"), key));
            }
        }
        if let Some(meta) = table.metatable() {
            children.push((format!("{path}<metatable>"), Value::Table(meta)));
        }
        self.tables.push(TableStat {
            id,
            site: self.origins[origin as usize].clone(),
            path,
            entries,
        });
        for (path, value) in children {
            self.push(value, origin, path);
        }
    }

    fn function(&mut self, func: Function, origin: u32, path: String) {
        let origin = match site_of(&func) {
            Some(site) if path.is_empty() => self.origin(site),
            _ => origin,
        };
        let name = if path.is_empty() {
            func.info().name.unwrap_or_else(|| "function".to_owned())
        } else {
            path
        };
        if let Some(env) = func.environment() {
            self.push(Value::Table(env), origin, "env".to_owned());
        }
        for (upvalue, value) in upvalues(self.lua, &func) {
            self.push(value, origin, format!("{name}↑{upvalue}"));
        }
    }

    /// A thread's locals and running functions, for the script its
    /// outermost frame is in.
    fn thread(&mut self, thread: &Thread, origin: Option<u32>) {
        let frames = frames(self.lua, thread);
        let site = frames
            .iter()
            .rev()
            .find(|f| f.site.is_some())
            .and_then(|f| f.site.clone());
        let fallback = match (site, origin) {
            (Some(site), _) => self.origin(site),
            (None, Some(origin)) => origin,
            (None, None) => self.origin(Site::default()),
        };
        for frame in frames {
            let origin = match frame.site {
                Some(site) => self.origin(site),
                None => fallback,
            };
            if let Some(func) = frame.func {
                self.push(Value::Function(func), origin, String::new());
            }
            for (name, value) in frame.locals {
                let path = match name.is_empty() {
                    true => "local".to_owned(),
                    false => format!("local {name}"),
                };
                self.push(value, origin, path);
            }
        }
    }
}

/// Tables, functions and threads lead somewhere; an instance's handle is
/// noted as reached.
fn walkable(value: &Value) -> bool {
    matches!(
        value,
        Value::Table(_) | Value::Function(_) | Value::Thread(_) | Value::UserData(_)
    )
}

/// What one walk found.
struct Walked {
    tables: Vec<TableStat>,
    instances: HashSet<Ref>,
    truncated: bool,
}

/// Walks every script's roots.
fn walk(lua: &Lua, engine: &Engine, handlers: Vec<(String, Function)>) -> Walked {
    let mut walk = Walk {
        lua,
        seen: HashSet::new(),
        queue: VecDeque::new(),
        origins: Vec::new(),
        tables: Vec::new(),
        instances: HashSet::new(),
        nodes: WALK_NODES,
        entries: WALK_ENTRIES,
        truncated: false,
    };
    // The shared globals and the libraries in them are no script's.
    let globals = lua.globals();
    walk.seen.insert(globals.to_pointer() as usize);
    let mut shared_roots = Vec::new();
    for (key, value) in globals.clone().pairs::<Value, Value>().flatten() {
        let name = key.as_string().map(|s| s.to_string_lossy().to_string());
        match name.as_deref() {
            Some(name @ ("_G" | "shared")) => shared_roots.push((name.to_owned(), value)),
            _ => {
                if matches!(value, Value::Table(_) | Value::Function(_)) {
                    walk.seen.insert(value.to_pointer() as usize);
                }
            }
        }
    }

    let (threads, waiting) = {
        let scheduler = engine.scheduler.borrow();
        let mut threads: Vec<Thread> = scheduler.owners.iter().map(|o| o.0.clone()).collect();
        threads.extend(scheduler.waiting.values().cloned());
        threads.extend(scheduler.delay_threads.iter().map(|d| d.0.clone()));
        let mut waiting: Vec<(Function, MultiValue)> =
            scheduler.delayed.values().cloned().collect();
        for (thread, args) in &scheduler.deferred {
            threads.push(thread.clone());
            if let Some(Value::Function(func)) = args.front() {
                waiting.push((func.clone(), args.clone()));
            }
        }
        (threads, waiting)
    };
    for thread in threads {
        if walk.seen.insert(thread.to_pointer() as usize) {
            walk.thread(&thread, None);
        }
    }
    for (func, args) in waiting {
        let site = site_of(&func).unwrap_or_default();
        let origin = walk.origin(site);
        for arg in args {
            walk.push(arg, origin, "queued call".to_owned());
        }
        walk.push(Value::Function(func), origin, "queued call".to_owned());
    }
    for (event, func) in handlers {
        let site = site_of(&func).unwrap_or_default();
        let origin = walk.origin(site);
        let event = event.split(':').next().unwrap_or(&event).to_owned();
        walk.push(Value::Function(func), origin, format!("{event} handler"));
    }
    let modules: Vec<(Ref, Value)> = engine.modules.borrow().loaded().collect();
    for (module, value) in modules {
        let script =
            crate::instance::tree::full_name(&engine.ctx.dom(), module).unwrap_or_default();
        let origin = walk.origin(Site { script, line: 0 });
        walk.push(value, origin, "module result".to_owned());
    }
    for (name, value) in shared_roots {
        let origin = walk.origin(Site {
            script: name.clone(),
            line: 0,
        });
        walk.push(value, origin, name);
    }
    walk.run();
    Walked {
        tables: walk.tables,
        instances: walk.instances,
        truncated: walk.truncated,
    }
}

/// A function's upvalues by name ("" where the chunk kept no names).
fn upvalues(lua: &Lua, func: &Function) -> Vec<(String, Value)> {
    // SAFETY: pushes each upvalue of the argument function after its
    // name, checking the stack first, then drops the function.
    let values = unsafe {
        lua.exec_raw::<MultiValue>(func.clone(), |state| {
            let mut n = 1;
            loop {
                ffi::lua_checkstack(state, 3);
                let name = ffi::lua_getupvalue(state, 1, n);
                if name.is_null() {
                    break;
                }
                ffi::lua_pushstring(state, name);
                ffi::lua_insert(state, -2);
                n += 1;
            }
            ffi::lua_remove(state, 1);
        })
    };
    pairs(values.unwrap_or_default())
}

fn pairs(values: MultiValue) -> Vec<(String, Value)> {
    let mut values = values.into_iter();
    let mut out = Vec::new();
    while let (Some(name), Some(value)) = (values.next(), values.next()) {
        let name = name
            .as_string()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        out.push((name, value));
    }
    out
}

struct Frame {
    site: Option<Site>,
    func: Option<Function>,
    locals: Vec<(String, Value)>,
}

fn number(value: Option<Value>) -> usize {
    match value {
        Some(Value::Integer(i)) => i.max(0) as usize,
        Some(Value::Number(n)) => n.max(0.0) as usize,
        _ => 0,
    }
}

/// A suspended thread's frames, innermost first: each one's script line,
/// function and named locals. A thread not started yet has one frame, its
/// stack.
fn frames(lua: &Lua, thread: &Thread) -> Vec<Frame> {
    // SAFETY: Luau's debug API on the argument thread, which is not
    // running (samples are taken between steps); every push is preceded
    // by a stack check, and values move to the calling state with
    // `lua_xmove`, as `debug.getinfo(thread, …)` does.
    let values = unsafe {
        lua.exec_raw::<MultiValue>(thread.clone(), |state| {
            let co = ffi::lua_tothread(state, 1);
            ffi::lua_settop(state, 0);
            if co.is_null() {
                return;
            }
            let mut level = 0;
            loop {
                let mut ar = std::mem::zeroed::<ffi::lua_Debug>();
                ffi::lua_checkstack(co, 2);
                if level > 200 || ffi::lua_getinfo(co, level, c"slf".as_ptr(), &mut ar) == 0 {
                    break;
                }
                ffi::lua_checkstack(state, 6);
                if ar.source.is_null() {
                    ffi::lua_pushstring(state, c"".as_ptr());
                } else {
                    ffi::lua_pushstring(state, ar.source);
                }
                ffi::lua_pushnumber(state, f64::from(ar.currentline));
                ffi::lua_xmove(co, state, 1);
                let count = ffi::lua_gettop(state) + 1;
                ffi::lua_pushnumber(state, 0.0);
                let mut n = 1;
                loop {
                    ffi::lua_checkstack(co, 2);
                    ffi::lua_checkstack(state, 3);
                    let name = ffi::lua_getlocal(co, level, n);
                    if name.is_null() {
                        break;
                    }
                    ffi::lua_pushstring(state, name);
                    ffi::lua_xmove(co, state, 1);
                    n += 1;
                }
                ffi::lua_pushnumber(state, f64::from(n - 1));
                ffi::lua_replace(state, count);
                level += 1;
            }
            if level == 0 {
                let top = ffi::lua_gettop(co);
                ffi::lua_checkstack(state, 4);
                ffi::lua_pushstring(state, c"".as_ptr());
                ffi::lua_pushnumber(state, 0.0);
                ffi::lua_pushnil(state);
                ffi::lua_pushnumber(state, f64::from(top));
                for i in 1..=top {
                    ffi::lua_checkstack(state, 3);
                    ffi::lua_checkstack(co, 1);
                    ffi::lua_pushstring(state, c"".as_ptr());
                    ffi::lua_pushvalue(co, i);
                    ffi::lua_xmove(co, state, 1);
                }
            }
        })
    };
    let mut values = values.unwrap_or_default().into_iter();
    let mut out = Vec::new();
    while let Some(source) = values.next() {
        let source = source
            .as_string()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let line = number(values.next());
        let func = match values.next() {
            Some(Value::Function(f)) => Some(f),
            _ => None,
        };
        let count = number(values.next());
        let mut locals = MultiValue::new();
        for _ in 0..count * 2 {
            locals.push_back(values.next().unwrap_or(Value::Nil));
        }
        let site = source
            .strip_prefix('=')
            .filter(|s| !internal_chunk(s))
            .map(|script| Site {
                script: script.to_owned(),
                line,
            });
        // The engine's own frames lend nothing of a script's.
        let internal = site.is_none() && !source.is_empty();
        out.push(Frame {
            site,
            func: func.filter(|_| !internal),
            locals: if internal { Vec::new() } else { pairs(locals) },
        });
    }
    out
}

/// Whether `series` rose at every step of its last [`GROWING`] values.
pub fn growing(series: &[usize]) -> bool {
    series.len() >= GROWING
        && series[series.len() - GROWING..]
            .windows(2)
            .all(|w| w[1] > w[0])
}

/// The last [`GROWING`] values of `f` over `history`, where every sample
/// has one.
fn series(history: &VecDeque<Sample>, f: impl Fn(&Sample) -> Option<usize>) -> Vec<usize> {
    history
        .iter()
        .rev()
        .take(GROWING)
        .map(f)
        .collect::<Option<Vec<_>>>()
        .map(|mut s| {
            s.reverse();
            s
        })
        .unwrap_or_default()
}

/// What in the newest sample looks like a leak: connections whose owner
/// was removed, or has been out of the place for two samples in a row;
/// and per-script connection counts, held instances of one class from one
/// site, and tables, that rose at each of the last [`GROWING`] samples.
/// The heap itself is never one: collection makes it rise and fall.
pub fn suspects(history: &VecDeque<Sample>) -> Vec<Suspect> {
    let Some(last) = history.back() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for stale in &last.stale {
        if stale.destroyed || stale.streak >= 2 {
            out.push(Suspect {
                kind: Kind::Signals,
                what: format!(
                    "{} on a {} {}",
                    stale.event,
                    if stale.destroyed {
                        "removed"
                    } else {
                        "nil-parented"
                    },
                    stale.owner
                ),
                site: Some(stale.site.clone()).filter(|s| !s.script.is_empty()),
                count: stale.count,
                growth: 0,
            });
        }
    }
    let rise = |s: &[usize]| s.last().copied().unwrap_or(0) - s.first().copied().unwrap_or(0);
    for script in &last.scripts {
        let s = series(history, |sample| {
            sample
                .scripts
                .iter()
                .find(|x| x.name == script.name)
                .map(|x| x.connections)
        });
        if growing(&s) {
            out.push(Suspect {
                kind: Kind::Signals,
                what: "connections keep growing".to_owned(),
                site: Some(Site {
                    script: script.name.clone(),
                    line: 0,
                }),
                count: script.connections,
                growth: rise(&s),
            });
        }
    }
    for group in &last.instances {
        let s = series(history, |sample| {
            sample
                .instances
                .iter()
                .find(|g| {
                    g.class == group.class && g.site == group.site && g.destroyed == group.destroyed
                })
                .map(|g| g.held)
        });
        if growing(&s) {
            out.push(Suspect {
                kind: Kind::Instances,
                what: format!(
                    "{} {}",
                    group.class,
                    if group.destroyed {
                        "destroyed, still held"
                    } else {
                        "parented to nil"
                    }
                ),
                site: group.site.clone(),
                count: group.held,
                growth: rise(&s),
            });
        }
    }
    for table in &last.tables {
        let s = series(history, |sample| {
            sample
                .tables
                .iter()
                .find(|t| t.id == table.id && t.path == table.path)
                .map(|t| t.entries)
        });
        if growing(&s) {
            out.push(Suspect {
                kind: Kind::Tables,
                what: table.path.clone(),
                site: Some(table.site.clone()),
                count: table.entries,
                growth: rise(&s),
            });
        }
    }
    out
}

/// One figure that changed between two samples.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRow {
    /// `LuaHeap`, `Signals`, `Instances` or `Luau tables`.
    pub category: &'static str,
    pub what: String,
    pub site: Option<Site>,
    pub before: usize,
    pub after: usize,
}

impl DiffRow {
    pub fn delta(&self) -> i64 {
        self.after as i64 - self.before as i64
    }
}

/// What changed from `before` to `after`, the biggest change first.
pub fn diff(before: &Sample, after: &Sample) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut add = |category, what: String, site: Option<Site>, a: usize, b: usize| {
        if a != b {
            rows.push(DiffRow {
                category,
                what,
                site,
                before: a,
                after: b,
            });
        }
    };
    let script_site = |name: &str| Site {
        script: name.to_owned(),
        line: 0,
    };
    let mut names: Vec<&str> = before
        .scripts
        .iter()
        .chain(&after.scripts)
        .map(|s| s.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let find = |s: &Sample| {
            s.scripts
                .iter()
                .find(|x| x.name == name)
                .cloned()
                .unwrap_or_default()
        };
        let (a, b) = (find(before), find(after));
        add(
            "LuaHeap",
            "heap".to_owned(),
            Some(script_site(name)),
            a.heap,
            b.heap,
        );
        add(
            "Signals",
            "connections".to_owned(),
            Some(script_site(name)),
            a.connections,
            b.connections,
        );
    }
    let key = |g: &InstanceGroup| (g.class.clone(), g.site.clone(), g.destroyed);
    let mut groups: Vec<_> = before
        .instances
        .iter()
        .chain(&after.instances)
        .map(key)
        .collect();
    groups.sort();
    groups.dedup();
    for (class, site, destroyed) in groups {
        let held = |s: &Sample| {
            s.instances
                .iter()
                .find(|g| g.class == class && g.site == site && g.destroyed == destroyed)
                .map_or(0, |g| g.held)
        };
        let what = format!(
            "{class} {}",
            if destroyed {
                "destroyed, held"
            } else {
                "nil-parented, held"
            }
        );
        add("Instances", what, site.clone(), held(before), held(after));
    }
    for table in &after.tables {
        if let Some(old) = before
            .tables
            .iter()
            .find(|t| t.id == table.id && t.path == table.path)
        {
            add(
                "Luau tables",
                table.path.clone(),
                Some(table.site.clone()),
                old.entries,
                table.entries,
            );
        }
    }
    rows.sort_by(|a, b| {
        b.delta()
            .abs()
            .cmp(&a.delta().abs())
            .then_with(|| a.category.cmp(b.category))
    });
    rows
}
