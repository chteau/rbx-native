//! Scripted bot players for load tests: real clients that join a session
//! the way Server & Clients' test players do (named `Bot1`, `Bot2`, ...,
//! negative UserIds) and play through [`Session::act`], the actions a
//! recording keeps, so a bot is nothing but a client with nobody at the
//! keys: it draws nothing and has no window.
//!
//! What each bot does is drawn from a [`Mix`] with the run's seed, so the
//! same seed gives the same bots doing the same things:
//!
//! - **idle**: stands where it spawned;
//! - **wander**: walks to random points it can reach (`PathfindingService`'s
//!   own search over its client's world), jumping where the path says to
//!   and wherever it gets stuck;
//! - **follow**: plays back a recorded player's input with a little jitter
//!   in when each action lands, over and over;
//! - **click**: clicks a random visible GUI button every few seconds;
//! - **remotes** (off unless asked for): fires the place's `RemoteEvent`s
//!   found at run time with arguments clients really sent them in this run
//!   (numbers nudged a little), or none when nobody has yet.

use std::collections::{BTreeSet, HashMap};

use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_lua::server::{PathRequest, Wire};

use crate::replay::{Action, Recording};
use crate::{Input, Peer, Session};

/// The window a bot's client lays its GUI out in.
const VIEWPORT: [f32; 2] = [1280.0, 720.0];
/// How far away a wander goal is, studs.
const WANDER: (f64, f64) = (16.0, 48.0);
/// Close enough to a waypoint to head for the next, studs (flat).
const REACHED: f64 = 2.0;
/// Frames between two looks at whether a walking bot got anywhere.
const STUCK_CHECK: u64 = 45;

/// How likely each behaviour is, as weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mix {
    pub idle: u32,
    pub wander: u32,
    pub follow: u32,
    pub click: u32,
    pub remotes: u32,
}

impl Default for Mix {
    /// Mostly walking about, some clicking, a few standing still; no
    /// recording to follow and no remote firing unless asked for.
    fn default() -> Self {
        Mix {
            idle: 1,
            wander: 6,
            follow: 0,
            click: 2,
            remotes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Behaviour {
    Idle,
    Wander,
    Follow,
    Click,
    Remotes,
}

impl Behaviour {
    pub fn name(self) -> &'static str {
        match self {
            Behaviour::Idle => "idle",
            Behaviour::Wander => "wander",
            Behaviour::Follow => "follow",
            Behaviour::Click => "click",
            Behaviour::Remotes => "remotes",
        }
    }
}

/// splitmix64 then xorshift64*: small, seedable, the same everywhere.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Rng((z ^ (z >> 31)) | 1)
    }

    /// Uniform in [0, 1).
    pub(crate) fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `lo..hi` (`lo` when empty).
    pub(crate) fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + (self.unit() * hi.saturating_sub(lo) as f64) as u64
    }
}

/// What a wandering bot is walking.
#[derive(Default)]
struct Walk {
    /// Waypoints and whether each asks for a jump.
    path: Vec<([f64; 3], bool)>,
    next: usize,
    /// The frame to plan the next walk on.
    plan_at: u64,
    /// Where it was at the last stuck check, and when that is due.
    checked: Option<([f64; 3], u64)>,
    stuck: u32,
}

/// A recorded player's input, as frames from its first.
struct Script {
    actions: Vec<(u64, Action)>,
    cursor: usize,
    /// The frame this pass started on, with each action's jitter so far.
    start: u64,
    last: u64,
}

struct Bot {
    name: String,
    behaviour: Behaviour,
    rng: Rng,
    keys: BTreeSet<String>,
    /// Releases waiting for the next frame: Space after a jump, the mouse
    /// button after a click.
    release: Vec<Input>,
    walk: Walk,
    script: Option<Script>,
    /// The frame of its next click or remote fire.
    next_at: u64,
    remotes: Vec<Ref>,
    listed_at: Option<u64>,
}

/// A load test's bots.
pub struct Bots {
    seed: u64,
    mix: Mix,
    /// Each recorded client's input, for followers.
    recorded: Vec<Vec<(u64, Action)>>,
    bots: Vec<Bot>,
    /// The number the next bot is named after.
    next: usize,
}

impl Bots {
    /// Bots drawn from `mix` with `seed`; followers play back `recording`'s
    /// players (without one they wander).
    pub fn new(seed: u64, mix: Mix, recording: Option<&Recording>) -> Bots {
        let mut recorded: Vec<Vec<(u64, Action)>> = Vec::new();
        if let Some(recording) = recording {
            let mut by_client: HashMap<usize, Vec<(u64, Action)>> = HashMap::new();
            for logged in &recording.actions {
                let Peer::Client(i) = logged.peer else {
                    continue;
                };
                if matches!(
                    logged.action,
                    Action::Input(_) | Action::Chat(_) | Action::Reset | Action::MenuOpen(_)
                ) {
                    by_client
                        .entry(i)
                        .or_default()
                        .push((logged.frame, logged.action.clone()));
                }
            }
            let mut clients: Vec<_> = by_client.into_iter().collect();
            clients.sort_by_key(|(i, _)| *i);
            for (_, mut actions) in clients {
                let first = actions.first().map_or(0, |(f, _)| *f);
                actions.iter_mut().for_each(|(f, _)| *f -= first);
                recorded.push(actions);
            }
        }
        Bots {
            seed,
            mix,
            recorded,
            bots: Vec::new(),
            next: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.bots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bots.is_empty()
    }

    /// Each bot's name and what it does, in joining order.
    pub fn roster(&self) -> Vec<(String, Behaviour)> {
        self.bots
            .iter()
            .map(|b| (b.name.clone(), b.behaviour))
            .collect()
    }

    fn draw(&self, rng: &mut Rng) -> Behaviour {
        let follow = if self.recorded.is_empty() {
            0
        } else {
            self.mix.follow
        };
        let wander = self.mix.wander
            + if self.recorded.is_empty() {
                self.mix.follow
            } else {
                0
            };
        let weights = [
            (Behaviour::Idle, self.mix.idle),
            (Behaviour::Wander, wander),
            (Behaviour::Follow, follow),
            (Behaviour::Click, self.mix.click),
            (Behaviour::Remotes, self.mix.remotes),
        ];
        let total: u32 = weights.iter().map(|w| w.1).sum();
        if total == 0 {
            return Behaviour::Idle;
        }
        let mut pick = rng.range(0, u64::from(total)) as u32;
        for (behaviour, weight) in weights {
            if pick < weight {
                return behaviour;
            }
            pick -= weight;
        }
        Behaviour::Idle
    }

    /// `count` more bots join, now.
    pub fn add(&mut self, session: &mut Session, count: usize) -> Result<(), String> {
        for _ in 0..count {
            let n = self.next;
            let mut rng = Rng::new(self.seed ^ (n as u64).wrapping_mul(0xA24B_AED4_963E_E407));
            let behaviour = self.draw(&mut rng);
            if behaviour == Behaviour::Remotes {
                session.keep_remote_args();
            }
            let name = format!("Bot{n}");
            let index = session.join(&name, -(n as i64), None)?;
            self.next += 1;
            session.act(Peer::Client(index), Action::Viewport(VIEWPORT));
            let frame = session.server.frame();
            let script = (behaviour == Behaviour::Follow).then(|| {
                let pick = rng.range(0, self.recorded.len() as u64) as usize;
                Script {
                    actions: self.recorded[pick].clone(),
                    cursor: 0,
                    start: frame + rng.range(0, 60),
                    last: 0,
                }
            });
            let next_at = frame + rng.range(30, 180);
            self.bots.push(Bot {
                name,
                behaviour,
                rng,
                keys: BTreeSet::new(),
                release: Vec::new(),
                walk: Walk::default(),
                script,
                next_at,
                remotes: Vec::new(),
                listed_at: None,
            });
        }
        Ok(())
    }

    /// The newest `count` bots leave.
    pub fn remove(&mut self, session: &mut Session, count: usize) {
        for _ in 0..count.min(self.bots.len()) {
            let bot = self.bots.pop().expect("counted");
            if let Some(index) = session.clients.iter().position(|c| c.name == bot.name) {
                session.leave(index);
            }
        }
    }

    /// Every bot's input for the session's next frame; call before each
    /// [`Session::step`].
    pub fn drive(&mut self, session: &mut Session) {
        let frame = session.server.frame();
        for bot in &mut self.bots {
            let Some(index) = session.clients.iter().position(|c| c.name == bot.name) else {
                continue;
            };
            let mut actions: Vec<Action> = bot.release.drain(..).map(Action::Input).collect();
            match bot.behaviour {
                Behaviour::Idle => {}
                Behaviour::Wander => wander(bot, session, index, frame, &mut actions),
                Behaviour::Follow => follow(bot, frame, &mut actions),
                Behaviour::Click => click(bot, session, index, frame, &mut actions),
                Behaviour::Remotes => fire(bot, session, index, frame, &mut actions),
            }
            for action in actions {
                session.act(Peer::Client(index), action);
            }
        }
    }
}

fn key(code: &str, down: bool) -> Action {
    Action::Input(Input::Key {
        code: code.into(),
        down,
    })
}

/// Holds exactly `wanted` of the movement keys.
fn hold(bot: &mut Bot, wanted: &[&'static str], actions: &mut Vec<Action>) {
    for code in ["W", "A", "S", "D"] {
        let want = wanted.contains(&code);
        if want != bot.keys.contains(code) {
            actions.push(key(code, want));
            if want {
                bot.keys.insert(code.to_owned());
            } else {
                bot.keys.remove(code);
            }
        }
    }
}

fn jump(bot: &mut Bot, actions: &mut Vec<Action>) {
    actions.push(key("Space", true));
    bot.release.push(Input::Key {
        code: "Space".into(),
        down: false,
    });
}

/// The movement keys that walk towards `dir` (flat, world space) with the
/// camera at `camera`: the default controls move relative to where the
/// camera looks.
pub(crate) fn keys_towards(dir: [f64; 2], camera: Option<&CFrameData>) -> Vec<&'static str> {
    let flat = |x: f64, z: f64| {
        let len = x.hypot(z);
        (len > 1e-6).then(|| [x / len, z / len])
    };
    let (look, right) = camera
        .and_then(|c| {
            let r = c.rotation.map(f64::from);
            Some((flat(-r[2], -r[8])?, flat(r[0], r[6])?))
        })
        .unwrap_or(([0.0, -1.0], [1.0, 0.0]));
    let Some(dir) = flat(dir[0], dir[1]) else {
        return Vec::new();
    };
    let forward = dir[0] * look[0] + dir[1] * look[1];
    let side = dir[0] * right[0] + dir[1] * right[1];
    // Eight directions, each 45 degrees wide.
    const EDGE: f64 = 0.383;
    let mut keys = Vec::new();
    if forward > EDGE {
        keys.push("W");
    } else if forward < -EDGE {
        keys.push("S");
    }
    if side > EDGE {
        keys.push("D");
    } else if side < -EDGE {
        keys.push("A");
    }
    keys
}

/// Where a client's own character's root part is.
fn root_position(dom: &WeakDom, character: Ref) -> Option<[f64; 3]> {
    let character = dom.get(character)?;
    let root = character.children().iter().find_map(|c| {
        let part = dom.get(*c)?;
        (part.name() == "HumanoidRootPart").then_some(part)
    })?;
    match root.properties().get("CFrame") {
        Some(Variant::CFrame(c)) => Some([
            f64::from(c.position.x),
            f64::from(c.position.y),
            f64::from(c.position.z),
        ]),
        _ => None,
    }
}

fn wander(bot: &mut Bot, session: &Session, index: usize, frame: u64, actions: &mut Vec<Action>) {
    let sim = &session.clients[index].sim;
    let Some((_, Some(character))) = sim.local_player() else {
        return;
    };
    let Some(at) = root_position(&sim.dom(), character) else {
        return;
    };
    let walk = &mut bot.walk;
    if walk.next >= walk.path.len() {
        hold(bot, &[], actions);
        if frame < bot.walk.plan_at {
            return;
        }
        let angle = bot.rng.unit() * std::f64::consts::TAU;
        let distance = WANDER.0 + bot.rng.unit() * (WANDER.1 - WANDER.0);
        let goal = [
            at[0] + angle.cos() * distance,
            at[1],
            at[2] + angle.sin() * distance,
        ];
        let request = PathRequest {
            start: at,
            goal,
            height: 5.0,
            can_jump: true,
            spacing: 4.0,
            costs: HashMap::new(),
        };
        let walk = &mut bot.walk;
        walk.path = sim
            .find_path(&request)
            .unwrap_or_default()
            .into_iter()
            .map(|w| (w.position, w.action == "Jump"))
            .collect();
        walk.next = 0;
        walk.stuck = 0;
        walk.checked = None;
        // Unreachable: another try in a moment; reached: a pause first.
        walk.plan_at = frame + bot.rng.range(30, 150);
        return;
    }
    let (target, jumps) = walk.path[walk.next];
    let dir = [target[0] - at[0], target[2] - at[2]];
    if dir[0].hypot(dir[1]) < REACHED {
        walk.next += 1;
        return;
    }
    let mut leap = jumps && dir[0].hypot(dir[1]) < 6.0;
    match walk.checked {
        Some((then, due)) if frame >= due => {
            let moved = (at[0] - then[0]).hypot(at[2] - then[2]);
            if moved < 1.0 {
                walk.stuck += 1;
                leap = true;
            } else {
                walk.stuck = 0;
            }
            walk.checked = Some((at, frame + STUCK_CHECK));
            if walk.stuck >= 3 {
                walk.path.clear();
                return;
            }
        }
        Some(_) => {}
        None => walk.checked = Some((at, frame + STUCK_CHECK)),
    }
    let camera = sim.camera().map(|(c, _)| c);
    let keys = keys_towards(dir, camera.as_ref());
    hold(bot, &keys, actions);
    if leap && !bot.keys.contains("Space") && bot.release.is_empty() {
        jump(bot, actions);
    }
}

fn follow(bot: &mut Bot, frame: u64, actions: &mut Vec<Action>) {
    let Some(script) = bot.script.as_mut() else {
        return;
    };
    while let Some((at, action)) = script.actions.get(script.cursor) {
        // Up to two frames either way, never before the previous action.
        let jitter = bot.rng.range(0, 5) as i64 - 2;
        let due = (script.start + at)
            .saturating_add_signed(jitter)
            .max(script.last);
        if due > frame {
            break;
        }
        if let Action::Input(Input::Key { code, down }) = action {
            match down {
                true => bot.keys.insert(code.clone()),
                false => bot.keys.remove(code),
            };
        }
        actions.push(action.clone());
        script.last = due;
        script.cursor += 1;
    }
    if script.cursor == script.actions.len() {
        // Again from the top in a second or three, nothing left held.
        for code in std::mem::take(&mut bot.keys) {
            actions.push(key(&code, false));
        }
        script.cursor = 0;
        script.start = frame + bot.rng.range(60, 180);
        script.last = 0;
    }
}

fn click(bot: &mut Bot, session: &Session, index: usize, frame: u64, actions: &mut Vec<Action>) {
    if frame < bot.next_at {
        return;
    }
    bot.next_at = frame + bot.rng.range(90, 300);
    let sim = &session.clients[index].sim;
    let dom = sim.dom();
    let gui = sim.gui();
    let buttons: Vec<[f32; 4]> = gui
        .laid
        .iter()
        .filter(|l| l.shown && l.rect[2] >= 1.0 && l.rect[3] >= 1.0)
        .filter(|l| {
            dom.get(l.referent)
                .is_some_and(|i| matches!(i.class(), "TextButton" | "ImageButton"))
        })
        .map(|l| l.rect)
        .collect();
    if buttons.is_empty() {
        return;
    }
    let rect = buttons[bot.rng.range(0, buttons.len() as u64) as usize];
    let position = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
    actions.push(Action::Input(Input::Move {
        position,
        delta: [0.0, 0.0],
    }));
    actions.push(Action::Input(Input::Button {
        button: 1,
        down: true,
    }));
    bot.release.push(Input::Button {
        button: 1,
        down: false,
    });
}

/// Every remote event in `dom`, in tree order.
fn remote_events(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else {
            continue;
        };
        if matches!(instance.class(), "RemoteEvent" | "UnreliableRemoteEvent") {
            out.push(r);
        }
        stack.extend(instance.children().iter().rev());
    }
    out
}

fn nudge(rng: &mut Rng, value: &Wire) -> Wire {
    match value {
        Wire::Number(n) if n.fract() != 0.0 => Wire::Number(n * (0.9 + rng.unit() * 0.2)),
        Wire::Table(entries) => Wire::Table(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), nudge(rng, v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn fire(bot: &mut Bot, session: &Session, index: usize, frame: u64, actions: &mut Vec<Action>) {
    if frame < bot.next_at {
        return;
    }
    bot.next_at = frame + bot.rng.range(30, 180);
    if bot.listed_at.is_none_or(|at| frame >= at + 600) {
        bot.remotes = remote_events(&session.clients[index].sim.dom());
        bot.listed_at = Some(frame);
    }
    if bot.remotes.is_empty() {
        return;
    }
    let remote = bot.remotes[bot.rng.range(0, bot.remotes.len() as u64) as usize];
    let seen = session.remote_args(remote);
    let args = match seen.len() {
        0 => Vec::new(),
        n => {
            let pick = &seen[bot.rng.range(0, n as u64) as usize];
            pick.iter().map(|a| nudge(&mut bot.rng, a)).collect()
        }
    };
    actions.push(Action::Fire { remote, args });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaw(degrees: f32) -> CFrameData {
        let (s, c) = degrees.to_radians().sin_cos();
        CFrameData {
            position: rbx_dom::Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c],
        }
    }

    #[test]
    fn keys_follow_the_camera() {
        // Looking down -Z (no turn): -Z is W, +X is D.
        assert_eq!(keys_towards([0.0, -1.0], Some(&yaw(0.0))), ["W"]);
        assert_eq!(keys_towards([1.0, 0.0], Some(&yaw(0.0))), ["D"]);
        assert_eq!(keys_towards([1.0, -1.0], Some(&yaw(0.0))), ["W", "D"]);
        // Turned 90 degrees left, the camera looks down -X: -X is W.
        assert_eq!(keys_towards([-1.0, 0.0], Some(&yaw(90.0))), ["W"]);
        assert_eq!(keys_towards([0.0, 1.0], Some(&yaw(90.0))), ["A"]);
        assert!(keys_towards([0.0, 0.0], None).is_empty());
    }

    #[test]
    fn the_mix_is_drawn_from_the_seed() {
        let bots = Bots::new(7, Mix::default(), None);
        let draw = |seed| {
            let mut rng = Rng::new(seed);
            (0..32).map(|_| bots.draw(&mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(draw(1), draw(1));
        assert_ne!(draw(1), draw(2));
        // No recording: nobody follows; remotes are off by default.
        let all = draw(3);
        assert!(!all.contains(&Behaviour::Follow) && !all.contains(&Behaviour::Remotes));
        assert!(all.contains(&Behaviour::Wander));
    }
}
