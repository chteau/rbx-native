//! Load tests: a headless Local Run server with up to [`MAX_BOTS`] bot
//! players (see [`crate::bots`]) joining on a ramp, measured every second
//! of game time, to see how a place holds up before launch.
//!
//! The bots' clients step on the same thread as the server, but on Roblox
//! they run on the players' own machines, so the server's figures leave
//! them out: [`Session::step_times`] splits each frame's time into the
//! server's part (its step, what arrives from the clients, what it
//! replicates) and the clients'. It is the thread's CPU time where the
//! platform has one, so a run at low priority on a busy machine still
//! measures the server, not the queue for the CPU.
//!
//! **Heartbeat** is the rate a dedicated server would keep at that cost: a
//! frame takes its 60 Hz slot or, when the server overruns it, its own
//! time. The server **keeps 60 Hz** while Heartbeat stays at or above
//! `min_hz` ([`DEFAULT_MIN_HZ`], a 5% allowance for a one-off hitch); the
//! **breaking point** is the bot count at the start of the first
//! [`SUSTAIN`] seconds in a row below it. `rbxlua loadtest` exits 1 when a
//! run finds one.
//!
//! DataStore and MemoryStore calls are counted per minute against the
//! budgets a live server gives that many players (creator docs, as
//! `datastore.luau`'s `RATES`). The run's DataStores are in memory only:
//! a load test never writes anyone's real data.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use rbx_dom::WeakDom;

use crate::bots::{Bots, Mix};
use crate::replay::Recording;
use crate::session::Traffic;
use crate::{Session, FRAME_HZ};

/// Bots a run starts with unless asked for more; more than this warns.
pub const DEFAULT_CAP: usize = 20;
/// The most bots a run takes.
pub const MAX_BOTS: usize = 50;
/// Heartbeat under this misses 60 Hz.
pub const DEFAULT_MIN_HZ: f64 = 57.0;
/// Seconds in a row under `min_hz` that make a breaking point.
pub const SUSTAIN: usize = 3;
const BUDGET_S: f64 = 1.0 / FRAME_HZ;

/// `step` more bots every `every` seconds until the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    pub step: usize,
    pub every: f64,
}

impl Ramp {
    /// `5/10s`: five more every ten seconds.
    pub fn parse(text: &str) -> Result<Ramp, String> {
        let bad = || format!("ramp '{text}' should look like 5/10s");
        let (step, every) = text.split_once('/').ok_or_else(bad)?;
        let step: usize = step.trim().parse().map_err(|_| bad())?;
        let every = parse_seconds(every).map_err(|_| bad())?;
        if step == 0 || every <= 0.0 {
            return Err(bad());
        }
        Ok(Ramp { step, every })
    }
}

/// `120s`, `2m` or a bare number of seconds.
pub fn parse_seconds(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let (number, scale) = if let Some(n) = text.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = text.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = text.strip_suffix('m') {
        (n, 60.0)
    } else {
        (text, 1.0)
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n * scale)
        .ok_or_else(|| format!("'{text}' isn't a duration (120s, 2m)"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub bots: usize,
    /// All at once when `None`.
    pub ramp: Option<Ramp>,
    /// Game seconds to run; until stopped when `None`.
    pub duration: Option<f64>,
    pub seed: u64,
    pub mix: Mix,
    pub min_hz: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bots: 10,
            ramp: Some(Ramp {
                step: 5,
                every: 10.0,
            }),
            duration: None,
            seed: 1,
            mix: Mix::default(),
            min_hz: DEFAULT_MIN_HZ,
        }
    }
}

impl Config {
    /// Refuses more than [`MAX_BOTS`]; the warning for more than
    /// [`DEFAULT_CAP`].
    pub fn check(&self) -> Result<Option<String>, String> {
        if self.bots > MAX_BOTS {
            return Err(format!("at most {MAX_BOTS} bots"));
        }
        Ok((self.bots > DEFAULT_CAP).then(|| {
            format!(
                "{} bots: each is a whole client on this machine, so it may get sluggish \
                 while the test runs",
                self.bots
            )
        }))
    }
}

/// One second of game time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// Game seconds at its end.
    pub t: f64,
    pub bots: usize,
    /// The server's step time per frame: mean and worst, ms.
    pub server_ms: f64,
    pub server_ms_max: f64,
    /// All the bots' clients together, ms per frame.
    pub clients_ms: f64,
    /// See the module docs.
    pub heartbeat_hz: f64,
    /// Frames this machine actually stepped per wall second, bots included.
    pub achieved_hz: f64,
    pub heap_mb: f64,
    pub client_heap_mb: f64,
    pub instances: usize,
    pub parts: usize,
    pub assemblies: usize,
    /// Calls in the last minute (in the first minute, the run so far).
    pub datastore_per_min: f64,
    pub memorystore_per_min: f64,
    /// Each DataStore request type's calls per minute.
    pub datastore_by_kind: BTreeMap<String, f64>,
    /// Remote packets and kilobytes a second: in to the server, out to the
    /// clients.
    pub remotes_in: f64,
    pub remotes_out: f64,
    pub kb_in: f64,
    pub kb_out: f64,
    /// This process's resident memory.
    pub rss_mb: f64,
}

/// A DataStore request type's budget per minute with `players` in the
/// server, `None` for a type without one.
pub fn datastore_budget(kind: &str, players: usize) -> Option<f64> {
    let (base, each) = match kind {
        "GetAsync" | "SetIncrementAsync" | "UpdateAsync" => (60.0, 10.0),
        "SetIncrementSortedAsync" | "OnUpdate" => (30.0, 5.0),
        "GetSortedAsync" | "ListAsync" | "GetVersionAsync" | "RemoveVersionAsync" => (5.0, 2.0),
        _ => return None,
    };
    Some(base + each * players as f64)
}

/// MemoryStoreService's requests per minute with `players` in the server.
pub fn memorystore_budget(players: usize) -> f64 {
    1000.0 + 100.0 * players as f64
}

/// The resident memory of this process, MB (0 where unknown).
pub fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

fn instance_count(dom: &WeakDom) -> usize {
    let mut count = 0;
    let mut stack: Vec<_> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        if let Some(instance) = dom.get(r) {
            count += 1;
            stack.extend(instance.children());
        }
    }
    count
}

#[derive(Default)]
struct Second {
    frames: u32,
    server: Duration,
    server_max: Duration,
    clients: Duration,
    /// Each frame's slot: 1/60 s or its server time when longer.
    slots: f64,
    wall: Option<Instant>,
}

pub struct LoadTest {
    pub session: Session,
    pub bots: Bots,
    config: Config,
    place: String,
    target: usize,
    next_ramp: f64,
    second: Second,
    samples: Vec<Sample>,
    /// The server's web requests so far at the start and each sample since,
    /// the last minute's worth.
    web: VecDeque<(f64, BTreeMap<String, u64>)>,
    traffic: Traffic,
    /// Resident memory before the first bot joined.
    rss_base: Option<f64>,
    streak: usize,
    breaking: Option<usize>,
    started: Instant,
    warning: Option<String>,
}

impl LoadTest {
    /// A headless server over `place` (named `name` in the report), its
    /// scripts started; the bots join from the first [`LoadTest::step`].
    /// Followers play back `recording`'s players.
    pub fn new(
        place: WeakDom,
        name: &str,
        config: Config,
        recording: Option<&Recording>,
    ) -> Result<LoadTest, String> {
        let warning = config.check()?;
        let mut session = Session::new(place)?;
        session.start();
        Ok(LoadTest {
            session,
            bots: Bots::new(config.seed, config.mix, recording),
            target: config.bots,
            config,
            place: name.into(),
            next_ramp: 0.0,
            second: Second::default(),
            samples: Vec::new(),
            web: VecDeque::from([(0.0, BTreeMap::new())]),
            traffic: Traffic::default(),
            rss_base: None,
            streak: 0,
            breaking: None,
            started: Instant::now(),
            warning,
        })
    }

    /// The warning [`Config::check`] gave, if any.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn target(&self) -> usize {
        self.target
    }

    /// Changes how many bots the run heads for (the ramp still paces a
    /// rise; a drop is at once).
    pub fn set_target(&mut self, bots: usize) {
        self.target = bots.min(MAX_BOTS);
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The bot count the server stopped keeping 60 Hz at, if it did.
    pub fn breaking_point(&self) -> Option<usize> {
        self.breaking
    }

    pub fn done(&self) -> bool {
        self.config
            .duration
            .is_some_and(|d| self.session.time() >= d - 1e-9)
    }

    /// The bot count the schedule wants this frame.
    fn wanted(&mut self) -> usize {
        let have = self.bots.len();
        if have >= self.target {
            return self.target;
        }
        match self.config.ramp {
            None => self.target,
            Some(ramp) => {
                let now = self.session.time();
                if now + 1e-9 < self.next_ramp {
                    return have;
                }
                self.next_ramp = now + ramp.every;
                (have + ramp.step).min(self.target)
            }
        }
    }

    /// One frame: bots join or leave as scheduled, each plays its part,
    /// the session steps; true when it closed a second's [`Sample`].
    pub fn step(&mut self) -> Result<bool, String> {
        let want = self.wanted();
        let have = self.bots.len();
        if want > have {
            self.rss_base.get_or_insert_with(resident_mb);
            self.bots.add(&mut self.session, want - have)?;
        } else if want < have {
            self.bots.remove(&mut self.session, have - want);
        }
        self.bots.drive(&mut self.session);
        self.second.wall.get_or_insert_with(Instant::now);
        self.session.step();
        let times = self.session.step_times();
        let second = &mut self.second;
        second.frames += 1;
        second.server += times.server;
        second.server_max = second.server_max.max(times.server);
        second.clients += times.clients;
        second.slots += times.server.as_secs_f64().max(BUDGET_S);
        if second.frames < FRAME_HZ as u32 {
            return Ok(false);
        }
        self.sample();
        Ok(true)
    }

    fn sample(&mut self) {
        let second = std::mem::take(&mut self.second);
        let frames = f64::from(second.frames);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0 / frames;
        let session = &self.session;
        let t = session.time();
        let bots = self.bots.len();
        let web = session.server.web_requests();
        let old = self.web.front().map(|(_, w)| w.clone()).unwrap_or_default();
        let per_min = |kind: &str| {
            let now = web.get(kind).copied().unwrap_or(0);
            let before = old.get(kind).copied().unwrap_or(0);
            now.saturating_sub(before) as f64
        };
        let datastore_by_kind: BTreeMap<String, f64> = web
            .keys()
            .filter(|k| k.as_str() != "MemoryStore")
            .map(|k| (k.clone(), per_min(k)))
            .collect();
        let traffic = session.traffic();
        let last = std::mem::replace(&mut self.traffic, traffic);
        let world = session.server.world();
        let sample = Sample {
            t,
            bots,
            server_ms: ms(second.server),
            server_ms_max: second.server_max.as_secs_f64() * 1000.0,
            clients_ms: ms(second.clients),
            heartbeat_hz: (frames / second.slots).min(FRAME_HZ),
            achieved_hz: second
                .wall
                .map_or(0.0, |w| frames / w.elapsed().as_secs_f64().max(1e-9)),
            heap_mb: session.server.heap_bytes() as f64 / 1_048_576.0,
            client_heap_mb: session
                .clients
                .iter()
                .map(|c| c.sim.heap_bytes())
                .sum::<usize>() as f64
                / 1_048_576.0,
            instances: instance_count(&session.server.dom()),
            parts: world.part_count(),
            assemblies: world.assembly_count(),
            datastore_per_min: datastore_by_kind.values().sum(),
            memorystore_per_min: per_min("MemoryStore"),
            datastore_by_kind,
            remotes_in: (traffic.packets_in - last.packets_in) as f64,
            remotes_out: (traffic.packets_out - last.packets_out) as f64,
            kb_in: (traffic.bytes_in - last.bytes_in) as f64 / 1000.0,
            kb_out: (traffic.bytes_out - last.bytes_out) as f64 / 1000.0,
            rss_mb: resident_mb(),
        };
        if self.web.len() == 61 {
            self.web.pop_front();
        }
        self.web.push_back((t, web));
        if sample.heartbeat_hz < self.config.min_hz {
            self.streak += 1;
            if self.streak == SUSTAIN && self.breaking.is_none() {
                let first = self.samples.len() + 1 - SUSTAIN;
                self.breaking = Some(self.samples.get(first).map_or(bots, |s| s.bots));
            }
        } else {
            self.streak = 0;
        }
        self.samples.push(sample);
    }

    /// Steps in real time (60 frames a wall second, never more; a machine
    /// that falls behind runs as fast as it can) until the duration is up
    /// or `control` says stop; `control` sees the run after every frame.
    pub fn run(
        &mut self,
        mut control: impl FnMut(&mut LoadTest, bool) -> bool,
    ) -> Result<(), String> {
        let mut start = Instant::now();
        let mut frames = 0u32;
        while !self.done() {
            let sampled = self.step()?;
            frames += 1;
            if !control(self, sampled) {
                break;
            }
            let due = start + Duration::from_secs_f64(f64::from(frames) / FRAME_HZ);
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            } else if now - due > Duration::from_millis(250) {
                // Behind: carry on from here rather than racing to catch up.
                start = now;
                frames = 0;
            }
        }
        Ok(())
    }

    /// The report so far.
    pub fn report(&self) -> Report {
        let most = self.samples.iter().map(|s| s.bots).max().unwrap_or(0);
        let busiest = self
            .samples
            .iter()
            .rev()
            .find(|s| s.bots == most && most > 0);
        let with_bots: Vec<&Sample> = self.samples.iter().filter(|s| s.bots > 0).collect();
        let per_bot = PerBot {
            client_ms: mean(with_bots.iter().map(|s| s.clients_ms / s.bots as f64)),
            client_heap_mb: busiest.map_or(0.0, |s| s.client_heap_mb / s.bots as f64),
            memory_mb: match (busiest, self.rss_base) {
                (Some(s), Some(base)) => ((s.rss_mb - base) / s.bots as f64).max(0.0),
                _ => 0.0,
            },
        };
        let mut budgets: BTreeMap<String, BudgetLine> = BTreeMap::new();
        for sample in &self.samples {
            let rows = sample
                .datastore_by_kind
                .iter()
                .filter_map(|(kind, rate)| {
                    Some((kind.clone(), *rate, datastore_budget(kind, sample.bots)?))
                })
                .chain(std::iter::once((
                    "MemoryStore".to_owned(),
                    sample.memorystore_per_min,
                    memorystore_budget(sample.bots),
                )));
            for (kind, rate, budget) in rows {
                let line = budgets.entry(kind.clone()).or_insert(BudgetLine {
                    kind,
                    peak_per_min: 0.0,
                    budget,
                });
                if rate / budget > line.peak_per_min / line.budget {
                    line.peak_per_min = rate;
                    line.budget = budget;
                }
            }
        }
        let mut roster: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (_, behaviour) in self.bots.roster() {
            *roster.entry(behaviour.name()).or_default() += 1;
        }
        Report {
            place: self.place.clone(),
            seed: self.config.seed,
            target: self.target,
            ramp: self.config.ramp,
            min_hz: self.config.min_hz,
            game_seconds: self.session.time(),
            wall_seconds: self.started.elapsed().as_secs_f64(),
            most_bots: most,
            breaking_point: self.breaking,
            per_bot,
            budgets: budgets
                .into_values()
                .filter(|l| l.peak_per_min > 0.0 || l.kind == "MemoryStore")
                .collect(),
            behaviours: roster.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
            samples: self.samples.clone(),
        }
    }

    /// Everyone leaves and the server closes; the final report.
    pub fn finish(mut self) -> Report {
        let report = self.report();
        self.session.shutdown();
        report
    }
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// What one bot costs this machine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerBot {
    /// Its client's step, ms per frame.
    pub client_ms: f64,
    /// Its client's Lua heap.
    pub client_heap_mb: f64,
    /// Resident memory per bot at the most bots.
    pub memory_mb: f64,
}

/// A web budget's busiest minute.
#[derive(Clone, Debug, PartialEq)]
pub struct BudgetLine {
    pub kind: String,
    pub peak_per_min: f64,
    /// The budget for the players in the server at that moment.
    pub budget: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub place: String,
    pub seed: u64,
    pub target: usize,
    pub ramp: Option<Ramp>,
    pub min_hz: f64,
    pub game_seconds: f64,
    pub wall_seconds: f64,
    pub most_bots: usize,
    pub breaking_point: Option<usize>,
    pub per_bot: PerBot,
    pub budgets: Vec<BudgetLine>,
    /// How many bots did each thing.
    pub behaviours: Vec<(String, usize)>,
    pub samples: Vec<Sample>,
}

fn peak(samples: &[Sample], f: impl Fn(&Sample) -> f64) -> f64 {
    samples.iter().map(f).fold(0.0, f64::max)
}

impl Report {
    /// The server kept 60 Hz the whole run.
    pub fn passed(&self) -> bool {
        self.breaking_point.is_none()
    }

    fn verdict(&self) -> String {
        match self.breaking_point {
            Some(n) => format!(
                "The server stopped keeping 60 Hz at {n} bots (Heartbeat under {} Hz for {SUSTAIN} s).",
                self.min_hz
            ),
            None => format!(
                "The server kept 60 Hz with up to {} bots (Heartbeat never under {} Hz for {SUSTAIN} s).",
                self.most_bots, self.min_hz
            ),
        }
    }

    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let s = &self.samples;
        let mut out = format!("# Load test: {}\n\n{}\n\n", self.place, self.verdict());
        let ramp = self.ramp.map_or("all at once".to_owned(), |r| {
            format!("+{} every {} s", r.step, r.every)
        });
        let behaviours = self
            .behaviours
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect::<Vec<_>>()
            .join(", ");
        let rows = [
            (
                "Bots (most / target)",
                format!("{} / {}", self.most_bots, self.target),
            ),
            ("Ramp", ramp),
            ("Behaviours", behaviours),
            ("Seed", self.seed.to_string()),
            (
                "Ran",
                format!(
                    "{:.0} s game time in {:.0} s",
                    self.game_seconds, self.wall_seconds
                ),
            ),
            (
                "Server step (peak mean / worst frame)",
                format!(
                    "{:.2} ms / {:.2} ms (budget 16.67 ms)",
                    peak(s, |x| x.server_ms),
                    peak(s, |x| x.server_ms_max)
                ),
            ),
            (
                "Lowest Heartbeat",
                format!(
                    "{:.1} Hz",
                    s.iter().map(|x| x.heartbeat_hz).fold(FRAME_HZ, f64::min)
                ),
            ),
            (
                "Peak Luau heap",
                format!("{:.1} MB", peak(s, |x| x.heap_mb)),
            ),
            (
                "Peak instances / parts",
                format!(
                    "{:.0} / {:.0}",
                    peak(s, |x| x.instances as f64),
                    peak(s, |x| x.parts as f64)
                ),
            ),
            (
                "Per bot (client step / heap / memory)",
                format!(
                    "{:.2} ms / {:.2} MB / {:.1} MB",
                    self.per_bot.client_ms, self.per_bot.client_heap_mb, self.per_bot.memory_mb
                ),
            ),
        ];
        out.push_str("| | |\n|---|---|\n");
        for (k, v) in rows {
            let _ = writeln!(out, "| {k} | {v} |");
        }
        out.push_str("\n## Web budgets (busiest minute)\n\n| Budget | Calls/min | Budget/min | |\n|---|---:|---:|---|\n");
        for line in &self.budgets {
            let over = if line.peak_per_min > line.budget {
                "over"
            } else {
                "ok"
            };
            let _ = writeln!(
                out,
                "| {} | {:.0} | {:.0} | {over} |",
                line.kind, line.peak_per_min, line.budget
            );
        }
        out.push_str(
            "\n## Timeline\n\n| t (s) | Bots | Server ms | Worst ms | Heartbeat Hz | Heap MB | Instances | Parts | DataStore/min | MemoryStore/min | Remotes in/s | Remotes out/s | KB in/s | KB out/s |\n|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        for x in s {
            let _ = writeln!(
                out,
                "| {:.0} | {} | {:.2} | {:.2} | {:.1} | {:.1} | {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {:.1} |",
                x.t,
                x.bots,
                x.server_ms,
                x.server_ms_max,
                x.heartbeat_hz,
                x.heap_mb,
                x.instances,
                x.parts,
                x.datastore_per_min,
                x.memorystore_per_min,
                x.remotes_in,
                x.remotes_out,
                x.kb_in,
                x.kb_out
            );
        }
        out
    }

    pub fn json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "place": self.place,
            "seed": self.seed,
            "target": self.target,
            "ramp": self.ramp.map(|r| json!({ "step": r.step, "every_s": r.every })),
            "min_hz": self.min_hz,
            "passed": self.passed(),
            "breaking_point": self.breaking_point,
            "verdict": self.verdict(),
            "game_seconds": self.game_seconds,
            "wall_seconds": self.wall_seconds,
            "most_bots": self.most_bots,
            "per_bot": {
                "client_ms": self.per_bot.client_ms,
                "client_heap_mb": self.per_bot.client_heap_mb,
                "memory_mb": self.per_bot.memory_mb,
            },
            "behaviours": self.behaviours.iter().map(|(k, n)| (k.clone(), json!(n))).collect::<serde_json::Map<_, _>>(),
            "budgets": self.budgets.iter().map(|l| json!({
                "kind": l.kind,
                "peak_per_min": l.peak_per_min,
                "budget_per_min": l.budget,
                "over": l.peak_per_min > l.budget,
            })).collect::<Vec<_>>(),
            "samples": self.samples.iter().map(|x| json!({
                "t": x.t,
                "bots": x.bots,
                "server_ms": x.server_ms,
                "server_ms_max": x.server_ms_max,
                "clients_ms": x.clients_ms,
                "heartbeat_hz": x.heartbeat_hz,
                "achieved_hz": x.achieved_hz,
                "heap_mb": x.heap_mb,
                "client_heap_mb": x.client_heap_mb,
                "instances": x.instances,
                "parts": x.parts,
                "assemblies": x.assemblies,
                "datastore_per_min": x.datastore_per_min,
                "datastore_by_kind": x.datastore_by_kind,
                "memorystore_per_min": x.memorystore_per_min,
                "remotes_in": x.remotes_in,
                "remotes_out": x.remotes_out,
                "kb_in": x.kb_in,
                "kb_out": x.kb_out,
                "rss_mb": x.rss_mb,
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests;
