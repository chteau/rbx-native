//! What a VM takes from outside the game, for Local Run's record and
//! replay: the wall clock, randomness, the web's answers and when they
//! came. A VM without a tape reads them as it always has; recording reads
//! them the same way and keeps what it read, replaying hands the kept
//! values back in the same order.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use mlua::{Lua, Table};

/// A background request's answer, delivered at the VM's `poll`-th poll.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub poll: u64,
    pub id: u32,
    pub result: Result<serde_json::Value, String>,
}

/// Everything one VM read from outside during a recording.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reel {
    /// `math.random`'s seed.
    pub seed: u32,
    /// Wall and CPU clock readings (`tick`, `os.time`, `os.clock`,
    /// `DateTime.now`, `GetServerTimeNow`, DataStore timestamps), in the
    /// order they were read.
    pub clock: Vec<f64>,
    /// Random draws (`Random.new()` seeds, GUIDs, spawn points), in order.
    pub entropy: Vec<u64>,
    pub answers: Vec<Answer>,
    /// Model assets downloaded by id (`LoadAsset`).
    pub assets: BTreeMap<u64, Result<Vec<u8>, String>>,
    /// The frame each background asset load (a sound, an animation, a
    /// video, by key) landed.
    pub loads: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Record,
    Replay,
}

struct State {
    mode: Mode,
    reel: Reel,
    clock: usize,
    entropy: usize,
    polls: u64,
    answer: usize,
    /// Replay: answers worked out again here, which win over the kept
    /// ones (a download's token is this process's).
    local: BTreeMap<u32, Result<serde_json::Value, String>>,
    /// Recording with [`Tape::seeded`]: the generator draws come from.
    rng: Option<u64>,
}

/// One VM's tape, shared with its background requests.
#[derive(Clone)]
pub struct Tape(Arc<Mutex<State>>);

impl Tape {
    /// A tape keeping what the VM reads, `math.random` seeded with `seed`.
    pub fn record(seed: u32) -> Tape {
        Tape::new(
            Mode::Record,
            Reel {
                seed,
                ..Reel::default()
            },
        )
    }

    /// A recording tape whose random draws (`Random.new()` seeds, GUIDs)
    /// come from `seed` too, not the system: the same seed draws the same.
    pub fn seeded(seed: u32) -> Tape {
        let tape = Tape::record(seed);
        tape.state().rng = Some(u64::from(seed));
        tape
    }

    /// A tape handing back what `reel` kept.
    pub fn replay(reel: Reel) -> Tape {
        Tape::new(Mode::Replay, reel)
    }

    fn new(mode: Mode, reel: Reel) -> Tape {
        Tape(Arc::new(Mutex::new(State {
            mode,
            reel,
            clock: 0,
            entropy: 0,
            polls: 0,
            answer: 0,
            local: BTreeMap::new(),
            rng: None,
        })))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn mode(&self) -> Mode {
        self.state().mode
    }

    /// What was kept so far.
    pub fn reel(&self) -> Reel {
        self.state().reel.clone()
    }

    pub(crate) fn replaying(&self) -> bool {
        self.mode() == Mode::Replay
    }

    /// A clock reading: `real()` kept, or the next kept one (the last
    /// again once they run out).
    pub fn clock(&self, real: impl FnOnce() -> f64) -> f64 {
        let mut s = self.state();
        match s.mode {
            Mode::Record => {
                let value = real();
                s.reel.clock.push(value);
                value
            }
            Mode::Replay => {
                let at = s.clock;
                s.clock += 1;
                s.reel
                    .clock
                    .get(at)
                    .or(s.reel.clock.last())
                    .copied()
                    .unwrap_or_else(real)
            }
        }
    }

    /// A random draw: `real()` kept, or the next kept one.
    pub fn entropy(&self, real: impl FnOnce() -> u64) -> u64 {
        let mut s = self.state();
        match s.mode {
            Mode::Record => {
                let value = match &mut s.rng {
                    Some(state) => splitmix(state),
                    None => real(),
                };
                s.reel.entropy.push(value);
                value
            }
            Mode::Replay => {
                let at = s.entropy;
                s.entropy += 1;
                s.reel.entropy.get(at).copied().unwrap_or_else(real)
            }
        }
    }

    /// One poll's finished requests: `done` kept, or the ones that were
    /// delivered at this poll when recording.
    pub(crate) fn poll(
        &self,
        done: Vec<(u32, Result<serde_json::Value, String>)>,
    ) -> Vec<(u32, Result<serde_json::Value, String>)> {
        let mut s = self.state();
        let poll = s.polls;
        s.polls += 1;
        match s.mode {
            Mode::Record => {
                s.reel
                    .answers
                    .extend(done.iter().map(|(id, result)| Answer {
                        poll,
                        id: *id,
                        result: result.clone(),
                    }));
                done
            }
            Mode::Replay => {
                let mut out = Vec::new();
                while let Some(answer) = s.reel.answers.get(s.answer) {
                    if answer.poll > poll {
                        break;
                    }
                    let (id, kept) = (answer.id, answer.result.clone());
                    s.answer += 1;
                    let result = s.local.remove(&id).unwrap_or(kept);
                    out.push((id, result));
                }
                out
            }
        }
    }

    /// Replay: `id`'s answer as this process worked it out.
    pub(crate) fn answer_here(&self, id: u32, result: Result<serde_json::Value, String>) {
        self.state().local.insert(id, result);
    }

    /// A downloaded model asset: `fetch()` kept, or the kept bytes.
    pub(crate) fn asset(
        &self,
        id: u64,
        fetch: impl FnOnce() -> Result<Vec<u8>, String>,
    ) -> Result<Vec<u8>, String> {
        let mode = self.mode();
        match mode {
            Mode::Record => {
                let result = fetch();
                self.state().reel.assets.insert(id, result.clone());
                result
            }
            Mode::Replay => self
                .state()
                .reel
                .assets
                .get(&id)
                .cloned()
                .unwrap_or_else(|| Err("HTTP 0 (not in the recording)".into())),
        }
    }

    /// Recording: a background load `key` landed on `frame` (the first
    /// time only).
    pub fn landed(&self, key: String, frame: u64) {
        self.state().reel.loads.entry(key).or_insert(frame);
    }

    /// Replay: the frame load `key` landed when recording, if it did.
    pub fn due(&self, key: &str) -> Option<u64> {
        self.state().reel.loads.get(key).copied()
    }
}

/// The VM's tape, if it has one.
pub(crate) fn of(lua: &Lua) -> Option<Tape> {
    lua.app_data_ref::<Tape>().map(|t| t.clone())
}

/// A clock reading through the VM's tape.
pub(crate) fn clock(lua: &Lua, real: impl FnOnce() -> f64) -> f64 {
    match of(lua) {
        Some(tape) => tape.clock(real),
        None => real(),
    }
}

/// A random draw through the VM's tape.
pub(crate) fn entropy(lua: &Lua, real: impl FnOnce() -> u64) -> u64 {
    match of(lua) {
        Some(tape) => tape.entropy(real),
        None => real(),
    }
}

/// SplitMix64's next number from `state`.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seconds since the Unix epoch, off the system clock.
pub(crate) fn epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Gives `lua` its tape: `math.random` takes the tape's seed, and
/// `os.time()`, `os.clock()` and `os.date()` without a time read the
/// clock through it.
pub(crate) fn install(lua: &Lua, tape: Tape) -> mlua::Result<()> {
    let seed = tape.state().reel.seed;
    lua.set_app_data(tape);
    let globals = lua.globals();
    globals
        .get::<Table>("math")?
        .get::<mlua::Function>("randomseed")?
        .call::<()>(seed)?;
    let os: Table = globals.get("os")?;
    let (time, date): (mlua::Function, mlua::Function) = (os.get("time")?, os.get("date")?);
    os.set_readonly(false);
    let wall = |lua: &Lua| clock(lua, epoch).floor();
    os.set(
        "time",
        lua.create_function(move |lua, t: Option<Table>| match t {
            Some(t) => time.call::<mlua::Value>(t),
            None => Ok(mlua::Value::Number(wall(lua))),
        })?,
    )?;
    os.set(
        "date",
        lua.create_function(
            move |lua, (format, at): (Option<mlua::Value>, Option<f64>)| {
                date.call::<mlua::Value>((format, at.unwrap_or_else(|| wall(lua))))
            },
        )?,
    )?;
    let cpu: mlua::Function = os.get("clock")?;
    os.set(
        "clock",
        lua.create_function(move |lua, ()| Ok(clock(lua, || cpu.call::<f64>(()).unwrap_or(0.0))))?,
    )?;
    os.set_readonly(true);
    Ok(())
}
