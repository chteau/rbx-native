//! A player who plays a Local Run from another computer: what crosses the
//! wire between the session's host and that player's editor. Bytes only;
//! the transport (the co-edit room's relay, sealed with the place key) is
//! the editor's, and nothing here is Roblox's own protocol.
//!
//! Upstream ([`Up`]) the player asks to join with their name, user id and
//! avatar, then sends the same [`Action`]s a recording keeps (keys, mouse,
//! screen size, chat, reset): small and many. Downstream ([`Down`]) the
//! host sends a [`Frame`] per frame shown: its client's DataModel changes
//! as [`Op`]s carrying the values they ended on (the whole tree first,
//! `reset`), and what the player's window shows besides (camera, core GUI
//! when it changed, chat, notices, printed lines). Referents are the
//! host's, so the player's copy of the tree names every instance as the
//! client does. Each message is LZ4-compressed.
//!
//! Sounds cross as events, never as audio: a frame says which sounds
//! started (and from where in their clip) or stopped, the sounds' own
//! properties come with the tree, and the player's editor plays them from
//! the same assets ([`Speaker`]), heard from its own camera. A slow link
//! gets fewer, merged frames ([`Frame::merge`]): the newest state, every
//! tree change still in order.
//!
//! [`Host`] is the session's side: offers to accept, the remote players,
//! what each has been sent. [`Viewer`] is the player's: their copy of the
//! client's DataModel, kept up by the frames, with their own character
//! moved ahead of them by their input ([`Predictor`]).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use rbx_assets::AssetFetcher;
use rbx_dom::{CFrameData, Change, Content, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::server::ChatLine;
use rbx_reflection::ReflectionDatabase;

use crate::audio::Voice;
use crate::gui::CoreGui;
use crate::humanoid::{flag, number};
use crate::replay::Action;
use crate::{Input, LineKind, MouseBehavior, Peer, Session};

/// Bumped whenever a message's layout changes. A peer on a later version
/// is refused, not misread; an earlier one's messages still read (version
/// 1 had no sounds and no input count in a frame).
pub const VERSION: u8 = 2;

mod codec;
pub use codec::{decode_down, decode_up, encode_down, encode_up};

/// Who sent or gets a message, as the transport names its ends.
pub type PeerId = u64;

/// From a remote player's editor to the session's host.
#[derive(Clone, Debug, PartialEq)]
pub enum Up {
    /// Asks to join as `name`, wearing `template` (a binary model; `None`
    /// for the default character).
    Ask {
        name: String,
        user_id: i64,
        template: Option<Vec<u8>>,
    },
    /// What the player did since the last message, in order.
    Actions(Vec<Action>),
    /// The test purchase dialog the frames showed closed.
    Purchase(bool),
    Leave,
    /// Frames went missing (the connection dropped): the whole tree again.
    Resync,
}

/// From the host to remote players.
#[derive(Clone, Debug, PartialEq)]
pub enum Down {
    /// To everyone, while a session takes players: who hosts it.
    Open {
        host: String,
    },
    /// The ask was taken: `player` is their `Player`, named `name`.
    Welcome {
        player: Ref,
        name: String,
    },
    Frame(Box<Frame>),
    /// The session is over for this player: ended, declined, kicked.
    Ended(String),
}

/// Where an instance goes, as `place` puts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Parent {
    /// A root of the tree (a service).
    Root,
    Under(Ref),
    /// Out of the tree, kept (an instance parented to nil).
    Nil,
}

/// One change to the player's copy of the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Add {
        at: Ref,
        parent: Parent,
        class: String,
        name: String,
        props: Vec<(String, Variant)>,
    },
    /// `None`: the property was removed. `Name` is the instance's name.
    Prop {
        at: Ref,
        name: String,
        value: Option<Variant>,
    },
    Move {
        at: Ref,
        parent: Parent,
    },
    /// The instance and everything under it.
    Remove(Ref),
    Class {
        at: Ref,
        class: String,
    },
}

/// One frame of a remote player's client, and of their window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// The ops build the tree from nothing.
    pub reset: bool,
    pub ops: Vec<Op>,
    pub camera: Option<(CFrameData, f32)>,
    pub mouse: MouseBehavior,
    pub player_gui: Option<Ref>,
    /// Only when it changed.
    pub core: Option<CoreGui>,
    pub chat: Vec<ChatLine>,
    pub notices: Vec<(String, String)>,
    /// The test purchase dialog: title, line.
    pub dialog: Option<(String, String)>,
    /// `Emit(n)` (`Some`) and `Clear` (`None`) calls.
    pub bursts: Vec<(Ref, Option<u32>)>,
    /// What the client printed.
    pub rows: Vec<(LineKind, String)>,
    pub paused: bool,
    /// Sounds that started (`Some`: at that many seconds into their clip,
    /// or a clock that wandered and jumps there) or stopped (`None`) since
    /// the last frame. A reset frame names every sound playing.
    pub sounds: Vec<(Ref, Option<f32>)>,
    /// How many `Up::Actions` messages from this player the host had
    /// applied: what their prediction is checked against.
    pub ack: u32,
}

impl Frame {
    /// `self` and then `later` as one frame, what a slow link sends in
    /// their place: tree changes in order (a property written in both
    /// goes once, as it ended), the newest camera and state, every line
    /// and event of both.
    pub fn merge(mut self, later: Frame) -> Frame {
        if later.reset {
            self.ops = later.ops;
            self.sounds = later.sounds;
        } else {
            self.ops.extend(later.ops);
            self.sounds.extend(later.sounds);
        }
        self.ops = compact(self.ops);
        let mut heard = HashSet::new();
        self.sounds.reverse();
        self.sounds.retain(|(sound, _)| heard.insert(*sound));
        self.sounds.reverse();
        self.reset |= later.reset;
        self.camera = later.camera;
        self.mouse = later.mouse;
        self.player_gui = later.player_gui;
        self.core = later.core.or(self.core);
        self.chat.extend(later.chat);
        self.notices.extend(later.notices);
        self.dialog = later.dialog;
        self.bursts.extend(later.bursts);
        self.rows.extend(later.rows);
        self.paused = later.paused;
        self.ack = later.ack;
        self
    }
}

/// `ops` without the property writes a later write of the same property
/// makes moot. Referents aren't reused, so nothing in between reads them.
fn compact(ops: Vec<Op>) -> Vec<Op> {
    let mut written = HashSet::new();
    let mut out: Vec<Op> = ops
        .into_iter()
        .rev()
        .filter(|op| match op {
            Op::Prop { at, name, .. } => written.insert((*at, name.clone())),
            _ => true,
        })
        .collect();
    out.reverse();
    out
}

/// What a remote player's client shows this frame, read off the host's
/// session.
pub struct Shot<'a> {
    pub dom: &'a WeakDom,
    /// The client's view changes since the last frame.
    pub changes: &'a [Change],
    /// `Simulation::sound_clocks`.
    pub sounds: Vec<(Ref, f64)>,
    /// The client's game time.
    pub time: f64,
}

/// How far a sound's clock may wander from where the last frame put it
/// before the next frame puts it right (the player's mixer jumps past
/// 0.12 s).
const SOUND_DRIFT: f64 = 0.1;

/// Whether `value` means anything on another computer: a script's
/// `EditableImage` or `EditableMesh` lives in the host's process only.
fn sendable(value: &Variant) -> bool {
    !matches!(value, Variant::Content(Content::Object(_)))
}

/// What one remote player's editor holds of their client's tree, as the
/// host keeps track of it, and the ops that bring it up to date.
#[derive(Default)]
pub struct Encoder {
    /// Instances the player has.
    known: HashSet<Ref>,
    /// The core GUI as last sent.
    core: Option<CoreGui>,
    /// The sounds the player plays: where each clock was sent, and when
    /// (game time).
    sounds: HashMap<Ref, (f64, f64)>,
}

/// Not in the game's tree: an `AnimationTrack`, a script's
/// `Instance.new` not parented yet, an old character. The test windows
/// leave them out too.
fn hidden(dom: &WeakDom, r: Ref) -> bool {
    dom.parent(r).is_none() || !dom.in_tree(r)
}

fn props(instance: &Instance) -> Vec<(String, Variant)> {
    instance
        .properties()
        .iter()
        .filter(|(_, v)| sendable(v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

impl Encoder {
    /// The whole tree, from nothing.
    pub fn snapshot(&mut self, dom: &WeakDom) -> Vec<Op> {
        self.known.clear();
        self.core = None;
        self.sounds.clear();
        let mut ops = Vec::new();
        for root in dom.root_refs() {
            self.add_subtree(dom, *root, &mut ops);
        }
        ops
    }

    /// What `changes` (a frame's, read back from `dom`) do to the player's
    /// copy: the same as the test windows' mirror does to theirs.
    pub fn ops(&mut self, dom: &WeakDom, changes: &[Change]) -> Vec<Op> {
        let mut ops = Vec::new();
        // A value written many times in a tick is sent once: as it ended.
        let mut written = HashSet::new();
        for change in changes {
            match change {
                Change::Property { referent, name } => {
                    let Some(instance) = dom.get(*referent) else {
                        continue;
                    };
                    if !self.known.contains(referent) || !written.insert((*referent, name)) {
                        continue;
                    }
                    let value = if name == "Name" {
                        Some(Variant::String(instance.name().to_owned()))
                    } else {
                        instance.properties().get(name.as_str()).cloned()
                    };
                    if value.as_ref().is_some_and(|v| !sendable(v)) {
                        continue;
                    }
                    ops.push(Op::Prop {
                        at: *referent,
                        name: name.clone(),
                        value,
                    });
                }
                Change::Added(r) if hidden(dom, *r) => {}
                Change::Added(r) => self.add_subtree(dom, *r, &mut ops),
                Change::Parent { referent, .. } if hidden(dom, *referent) => {
                    if self.known.contains(referent) {
                        self.forget(dom, *referent);
                        ops.push(Op::Remove(*referent));
                    }
                }
                Change::Parent { referent, .. } => {
                    if dom.get(*referent).is_none() {
                        continue;
                    }
                    if !self.known.contains(referent) {
                        self.add_subtree(dom, *referent, &mut ops);
                    } else if let Some(parent) = self.parent(dom, *referent) {
                        ops.push(Op::Move {
                            at: *referent,
                            parent,
                        });
                    }
                }
                Change::Removed(r) => {
                    if self.known.remove(r) {
                        ops.push(Op::Remove(*r));
                    }
                }
                Change::Class(r) => {
                    if let (Some(instance), true) = (dom.get(*r), self.known.contains(r)) {
                        ops.push(Op::Class {
                            at: *r,
                            class: instance.class().to_owned(),
                        });
                    }
                }
            }
        }
        ops
    }

    /// The sound events since the last frame: sounds the player has that
    /// started or stopped, and clocks that wandered from where they were
    /// sent (a seek, a speed change, a loop).
    fn sounds(
        &mut self,
        dom: &WeakDom,
        clocks: &[(Ref, f64)],
        time: f64,
    ) -> Vec<(Ref, Option<f32>)> {
        let db = ReflectionDatabase::shared();
        let playing: HashSet<Ref> = clocks.iter().map(|(sound, _)| *sound).collect();
        let mut out: Vec<(Ref, Option<f32>)> = Vec::new();
        self.sounds.retain(|sound, _| {
            let on = playing.contains(sound);
            if !on {
                out.push((*sound, None));
            }
            on
        });
        out.sort_by_key(|(sound, _)| sound.value());
        for &(sound, position) in clocks {
            if !self.known.contains(&sound) {
                continue;
            }
            let speed = number(dom, db, sound, "PlaybackSpeed").max(0.0);
            let due = self.sounds.get(&sound).is_none_or(|(at, then)| {
                (at + (time - then) * speed - position).abs() > SOUND_DRIFT
            });
            if due {
                self.sounds.insert(sound, (position, time));
                out.push((sound, Some(position as f32)));
            }
        }
        out
    }

    /// `core` when it differs from what was sent last.
    pub fn core(&mut self, core: CoreGui) -> Option<CoreGui> {
        if self.core.as_ref() == Some(&core) {
            return None;
        }
        self.core = Some(core.clone());
        Some(core)
    }

    /// Where `r` goes in the player's copy; `None` when its parent isn't
    /// there (it follows once the parent comes).
    fn parent(&self, dom: &WeakDom, r: Ref) -> Option<Parent> {
        match dom.parent(r) {
            Some(p) if self.known.contains(&p) => Some(Parent::Under(p)),
            Some(_) => None,
            None if dom.root_refs().contains(&r) => Some(Parent::Root),
            None => Some(Parent::Nil),
        }
    }

    fn add_subtree(&mut self, dom: &WeakDom, root: Ref, ops: &mut Vec<Op>) {
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().rev().copied());
            if self.known.contains(&at) {
                continue;
            }
            let Some(parent) = self.parent(dom, at) else {
                continue;
            };
            self.known.insert(at);
            ops.push(Op::Add {
                at,
                parent,
                class: instance.class().to_owned(),
                name: instance.name().to_owned(),
                props: props(instance),
            });
        }
    }

    fn forget(&mut self, dom: &WeakDom, root: Ref) {
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            self.known.remove(&at);
            if let Some(instance) = dom.get(at) {
                stack.extend(instance.children().iter().copied());
            }
        }
    }
}

/// A remote player's copy of their client's DataModel.
pub struct Viewer {
    dom: Rc<RefCell<WeakDom>>,
    /// Moves their own character ahead of the frames.
    pub predictor: Predictor,
}

impl Default for Viewer {
    fn default() -> Self {
        Viewer {
            dom: Rc::new(RefCell::new(WeakDom::new())),
            predictor: Predictor::default(),
        }
    }
}

fn place(dom: &mut WeakDom, at: Ref, parent: Parent) {
    match parent {
        Parent::Under(p) if dom.get(p).is_some() => dom.set_parent(at, Some(p)),
        Parent::Under(_) => {}
        Parent::Root => dom.set_parent(at, None),
        Parent::Nil => dom.unparent(at),
    }
}

impl Viewer {
    pub fn dom_rc(&self) -> Rc<RefCell<WeakDom>> {
        self.dom.clone()
    }

    /// Applies `frame`'s ops; the changes they made, for whatever shows
    /// the copy (a reset starts the copy over, and changes nothing).
    pub fn apply(&mut self, frame: &Frame) -> Vec<Change> {
        let mut dom = self.dom.borrow_mut();
        // The frame's values are the host's: the prediction goes back on
        // after.
        let mut changes = self.predictor.unshift(&mut dom);
        if frame.reset {
            *dom = WeakDom::new();
            self.predictor.restart();
        }
        self.predictor.saw(frame.ack);
        for op in &frame.ops {
            match op {
                Op::Add {
                    at,
                    parent,
                    class,
                    name,
                    props,
                } => {
                    if dom.get(*at).is_some() {
                        continue;
                    }
                    let mut instance = Instance::new(*at, class.as_str(), name.as_str());
                    instance.properties_mut().extend(props.iter().cloned());
                    dom.insert(instance);
                    place(&mut dom, *at, *parent);
                    changes.push(Change::Added(*at));
                }
                Op::Prop { at, name, value } => {
                    let done = match value {
                        Some(Variant::String(text)) if name == "Name" => {
                            dom.set_name(*at, text).is_ok()
                        }
                        Some(value) => dom.set_property(*at, name, value.clone()).is_ok(),
                        None => dom.remove_property(*at, name).is_ok(),
                    };
                    if done {
                        changes.push(Change::Property {
                            referent: *at,
                            name: name.clone(),
                        });
                    }
                }
                Op::Move { at, parent } => {
                    if dom.get(*at).is_some() {
                        let old = dom.parent(*at);
                        place(&mut dom, *at, *parent);
                        changes.push(Change::Parent {
                            referent: *at,
                            old,
                            new: dom.parent(*at),
                        });
                    }
                }
                Op::Remove(at) => {
                    changes.extend(dom.remove(*at).into_iter().map(Change::Removed));
                }
                Op::Class { at, class } => {
                    if dom.set_class(*at, class).is_ok() {
                        changes.push(Change::Class(*at));
                    }
                }
            }
        }
        dom.take_changes();
        if frame.reset {
            changes.clear();
        }
        changes
    }

    /// The player's own input as it goes up: their character moves on it
    /// before the host's frames show it.
    pub fn input(&mut self, actions: &[Action]) {
        for action in actions {
            self.predictor.input(action);
        }
    }

    /// Moves `player`'s character `dt` seconds on along their input,
    /// checked against the frames applied since the last call: the parts
    /// it moved in the copy, and `camera` (the frame's) moved with it.
    pub fn predict(
        &mut self,
        player: Ref,
        camera: Option<(CFrameData, f32)>,
        dt: f64,
    ) -> (Vec<Change>, Option<(CFrameData, f32)>) {
        let mut dom = self.dom.borrow_mut();
        self.predictor.update(&mut dom, player, camera, dt)
    }
}

/// A correction this big (studs) is shown at once, not blended in: a
/// respawn, a teleport, a push the player couldn't have foreseen.
const SNAP: f64 = 4.0;
/// How long a smaller correction takes to blend in, as a time constant.
const BLEND: f64 = 0.1;
/// Predicted positions kept for corrections, at most (ten seconds of
/// input at 60 messages a second).
const HISTORY: usize = 600;

/// The player's own character, moved on their input before the host's
/// frames show it, and pulled back to the host's as frames come (client-
/// side prediction). Their walk is the control script's: the movement
/// keys, relative to the camera, at the humanoid's `WalkSpeed`; a jump
/// leaves at the humanoid's jump speed under the workspace's gravity.
/// Each `Up::Actions` sent keeps where the root was predicted then; a
/// frame says how many the host had applied, and the root it shows is
/// compared with the prediction kept for that many. The difference moves
/// the prediction (and what's kept after it) and is blended into what's
/// shown, at once when it's over [`SNAP`].
#[derive(Default)]
pub struct Predictor {
    /// Off: the frames are shown as they come.
    pub off: bool,
    /// Forward, back, left and right held.
    held: [bool; 4],
    jump: bool,
    /// `Up::Actions` messages sent.
    sent: u32,
    /// What the newest frame said the host had applied, and whether it's
    /// been checked yet.
    ack: u32,
    fresh: bool,
    /// Where the root was predicted as each message left.
    history: VecDeque<(u32, [f64; 3])>,
    /// The predicted root position.
    at: Option<[f64; 3]>,
    /// In a predicted jump: the upward speed and the floor it left.
    air: Option<(f64, f64)>,
    /// Of the corrections so far, what isn't shown yet.
    lag: [f64; 3],
    character: Option<Ref>,
    /// Parts moved in the copy, and by how much.
    shifted: Vec<Ref>,
    offset: [f64; 3],
}

fn child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == class))
}

fn cframe_of(dom: &WeakDom, part: Ref) -> Option<CFrameData> {
    match dom.get(part)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(*c),
        _ => None,
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn vec3(p: Vector3Data) -> [f64; 3] {
    [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
}

fn moved(c: CFrameData, by: [f64; 3]) -> CFrameData {
    CFrameData {
        position: Vector3Data {
            x: (f64::from(c.position.x) + by[0]) as f32,
            y: (f64::from(c.position.y) + by[1]) as f32,
            z: (f64::from(c.position.z) + by[2]) as f32,
        },
        rotation: c.rotation,
    }
}

/// `x`, `z` on the ground along `right` and `back` (a camera's columns),
/// flattened and made unit.
fn flat(x: f64, z: f64) -> Option<[f64; 2]> {
    let n = x.hypot(z);
    (n > 1e-6).then(|| [x / n, z / n])
}

impl Predictor {
    /// Starts over: a new tree, a new character.
    pub fn restart(&mut self) {
        let (off, held, jump, sent) = (self.off, self.held, self.jump, self.sent);
        *self = Predictor {
            off,
            held,
            jump,
            sent,
            ack: sent,
            ..Predictor::default()
        };
    }

    /// One of the player's actions on its way up.
    pub fn input(&mut self, action: &Action) {
        match action {
            Action::Input(Input::Key { code, down }) => {
                let slot = match code.as_str() {
                    "W" | "Up" => 0,
                    "S" | "Down" => 1,
                    "A" | "Left" => 2,
                    "D" | "Right" => 3,
                    "Space" => {
                        self.jump = *down;
                        return;
                    }
                    _ => return,
                };
                self.held[slot] = *down;
            }
            // The menu takes the keys: the control script lets go too.
            Action::MenuOpen(true) | Action::Reset => {
                self.held = [false; 4];
                self.jump = false;
            }
            _ => {}
        }
    }

    /// Whether the player should send even with nothing new: their
    /// character is moving on input, and each message is a point the
    /// host's frames are checked against.
    pub fn moving(&self) -> bool {
        !self.off && (self.held.iter().any(|h| *h) || self.jump || self.air.is_some())
    }

    /// An `Up::Actions` message left: where the root is predicted now is
    /// what the host should show once it applied it.
    pub fn sent(&mut self) {
        self.sent = self.sent.wrapping_add(1);
        if let Some(at) = self.at {
            self.history.push_back((self.sent, at));
            if self.history.len() > HISTORY {
                self.history.pop_front();
            }
        }
    }

    fn saw(&mut self, ack: u32) {
        self.ack = ack;
        self.fresh = true;
    }

    /// Puts the parts moved back where the frames had them; the parts.
    fn unshift(&mut self, dom: &mut WeakDom) -> Vec<Change> {
        let back = self.offset.map(|v| -v);
        std::mem::take(&mut self.shifted)
            .into_iter()
            .filter_map(|part| {
                let c = cframe_of(dom, part)?;
                dom.set_property(part, "CFrame", Variant::CFrame(moved(c, back)))
                    .ok()?;
                Some(Change::Property {
                    referent: part,
                    name: "CFrame".into(),
                })
            })
            .collect()
    }

    fn update(
        &mut self,
        dom: &mut WeakDom,
        player: Ref,
        camera: Option<(CFrameData, f32)>,
        dt: f64,
    ) -> (Vec<Change>, Option<(CFrameData, f32)>) {
        let mut changes = self.unshift(dom);
        let character = match dom
            .get(player)
            .and_then(|p| p.properties().get("Character"))
        {
            Some(Variant::Ref(r)) if dom.get(*r).is_some() => Some(*r),
            _ => None,
        };
        let root = character.and_then(|c| {
            dom.get(c)?
                .children()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.name() == "HumanoidRootPart"))
        });
        let (Some(character), Some(host)) = (character, root.and_then(|r| cframe_of(dom, r)))
        else {
            self.restart();
            return (changes, camera);
        };
        let host = vec3(host.position);
        if self.character != Some(character) {
            self.restart();
            self.character = Some(character);
        }
        if self.off {
            return (changes, camera);
        }
        if self.at.is_none() {
            self.at = Some(host);
            self.history.push_back((self.sent, host));
        }
        if std::mem::take(&mut self.fresh) {
            self.reconcile(host);
        }
        self.step(dom, character, camera.map(|(c, _)| c), dt);
        let shown = sub(self.at.unwrap_or(host), self.lag);
        let offset = sub(shown, host);
        if length(offset) < 1e-4 {
            return (changes, camera);
        }
        let db = ReflectionDatabase::shared();
        let mut stack = vec![character];
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().copied());
            if !db.is_subclass_of(instance.class(), "BasePart") {
                continue;
            }
            let Some(c) = cframe_of(dom, at) else {
                continue;
            };
            if dom
                .set_property(at, "CFrame", Variant::CFrame(moved(c, offset)))
                .is_ok()
            {
                self.shifted.push(at);
                if !changes
                    .iter()
                    .any(|ch| matches!(ch, Change::Property { referent, .. } if *referent == at))
                {
                    changes.push(Change::Property {
                        referent: at,
                        name: "CFrame".into(),
                    });
                }
            }
        }
        self.offset = offset;
        (changes, camera.map(|(c, fov)| (moved(c, offset), fov)))
    }

    /// The host's root, as of the newest frame, against where it was
    /// predicted for the input the host had then.
    fn reconcile(&mut self, host: [f64; 3]) {
        while self.history.len() > 1 && self.history.front().is_some_and(|(n, _)| *n != self.ack) {
            self.history.pop_front();
        }
        let Some(&(n, then)) = self.history.front() else {
            return;
        };
        if n != self.ack {
            return;
        }
        let error = sub(host, then);
        for (_, p) in &mut self.history {
            *p = add(*p, error);
        }
        self.at = self.at.map(|at| add(at, error));
        self.lag = if length(error) > SNAP {
            [0.0; 3]
        } else {
            add(self.lag, error)
        };
        if self.air.is_some() && error[1].abs() > SNAP {
            self.air = None;
        }
    }

    fn step(&mut self, dom: &WeakDom, character: Ref, camera: Option<CFrameData>, dt: f64) {
        let db = ReflectionDatabase::shared();
        let decay = (-dt / BLEND).exp();
        self.lag = self.lag.map(|v| v * decay);
        let Some(humanoid) = child_of_class(dom, character, "Humanoid") else {
            return;
        };
        let able = number(dom, db, humanoid, "Health") > 0.0
            && !flag(dom, db, humanoid, "Sit")
            && !flag(dom, db, humanoid, "PlatformStand");
        let Some(mut at) = self.at else { return };
        let h = |i: usize| f64::from(u8::from(self.held[i]));
        let (x, z) = (h(3) - h(2), h(1) - h(0));
        let amount = x.hypot(z).min(1.0);
        if able && amount > 0.0 {
            let r = camera.map_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], |c| {
                c.rotation
            });
            let right = flat(f64::from(r[0]), f64::from(r[6])).unwrap_or([1.0, 0.0]);
            let back = flat(f64::from(r[2]), f64::from(r[8])).unwrap_or([0.0, 1.0]);
            if let Some(dir) = flat(right[0] * x + back[0] * z, right[1] * x + back[1] * z) {
                let step = number(dom, db, humanoid, "WalkSpeed").max(0.0) * amount * dt;
                at[0] += dir[0] * step;
                at[2] += dir[1] * step;
            }
        }
        let gravity = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
            .map_or(196.2, |w| number(dom, db, w, "Gravity"));
        if self.jump && self.air.is_none() && able {
            let speed = if flag(dom, db, humanoid, "UseJumpPower") {
                crate::humanoid::JUMP_POWER_SCALE * number(dom, db, humanoid, "JumpPower")
            } else {
                (2.0 * gravity * number(dom, db, humanoid, "JumpHeight").max(0.0)).sqrt()
            };
            self.air = Some((speed, at[1]));
        }
        if let Some((speed, floor)) = self.air {
            at[1] += speed * dt - 0.5 * gravity * dt * dt;
            let speed = speed - gravity * dt;
            self.air = if at[1] <= floor && speed < 0.0 {
                at[1] = floor;
                None
            } else {
                Some((speed, floor))
            };
        }
        self.at = Some(at);
    }
}

/// What a remote player hears: the sounds their client plays, started,
/// moved and stopped by the frames, each one's clock kept here between
/// them, played from the same assets (cached or fetched on this computer)
/// as their camera hears them.
pub struct Speaker {
    sounds: crate::sound::Sounds,
    clocks: HashMap<Ref, f64>,
}

impl Speaker {
    /// `fetcher` gets the sounds not in this computer's cache.
    pub fn new(fetcher: Option<Arc<dyn AssetFetcher>>) -> Speaker {
        let mut sounds = crate::sound::Sounds::default();
        sounds.fetcher = fetcher;
        Speaker {
            sounds,
            clocks: HashMap::new(),
        }
    }

    /// A frame's sound events (a reset frame starts over).
    pub fn hear(&mut self, frame: &Frame) {
        if frame.reset {
            self.clocks.clear();
        }
        for (sound, at) in &frame.sounds {
            match at {
                Some(at) => self.clocks.insert(*sound, f64::from(*at)),
                None => self.clocks.remove(sound),
            };
        }
    }

    /// The clocks `dt` seconds on, at each sound's `PlaybackSpeed`; a
    /// looped one wraps.
    pub fn advance(&mut self, dom: &WeakDom, dt: f64) {
        let db = ReflectionDatabase::shared();
        self.clocks.retain(|sound, _| dom.get(*sound).is_some());
        for (sound, at) in &mut self.clocks {
            *at += dt * number(dom, db, *sound, "PlaybackSpeed").max(0.0);
            if flag(dom, db, *sound, "Looped") {
                if let Some(clip) = self.sounds.clip_of(dom, *sound) {
                    let length = clip.seconds();
                    if length > 0.0 {
                        *at %= length;
                    }
                }
            }
        }
    }

    /// What plays now, heard from `camera` (or wherever the place's
    /// `SoundService` puts the listener).
    pub fn voices(&mut self, dom: &WeakDom, camera: Option<CFrameData>) -> Vec<Voice> {
        let db = ReflectionDatabase::shared();
        let listener = crate::sound::listener(dom, db, camera);
        let clocks: Vec<(Ref, f64)> = self.clocks.iter().map(|(s, at)| (*s, *at)).collect();
        self.sounds.voices_at(dom, db, listener.as_ref(), &clocks)
    }

    /// The sounds playing, and where.
    pub fn playing(&self) -> &HashMap<Ref, f64> {
        &self.clocks
    }
}

/// An ask to join, waiting for the host.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub from: PeerId,
    pub name: String,
    pub user_id: i64,
    template: Option<Vec<u8>>,
}

/// A player in the session from another computer.
pub struct Guest {
    pub from: PeerId,
    pub player: Ref,
    encoder: Encoder,
    /// Sent their first frame (the whole tree).
    started: bool,
    /// What they asked with, to carry them through a teleport.
    offer: Offer,
    /// `Up::Actions` messages applied.
    acked: u32,
    /// A frame held back while the link is slow, sent merged with the
    /// next.
    held: Option<Frame>,
}

/// What a message from a remote player came to, for the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A new ask to join, by this name.
    Offered(String),
    /// A remote player left.
    Left(Ref),
    /// A remote player closed the purchase dialog.
    Purchase { player: Ref, bought: bool },
}

/// The session's side of its remote players.
#[derive(Default)]
pub struct Host {
    pub offers: Vec<Offer>,
    pub guests: Vec<Guest>,
}

fn index_of(session: &Session, player: Ref) -> Option<usize> {
    session.clients.iter().position(|c| c.player == player)
}

/// What a remote player may do to their client: what their own window
/// would, nothing that runs the session (start, join, network, purchase
/// outcomes the host decides).
fn allowed(action: &Action) -> bool {
    matches!(
        action,
        Action::Input(_)
            | Action::Viewport(_)
            | Action::SafeInsets(_)
            | Action::MenuOpen(_)
            | Action::Chat(_)
            | Action::Reset
    )
}

impl Host {
    pub fn guest(&self, player: Ref) -> Option<&Guest> {
        self.guests.iter().find(|g| g.player == player)
    }

    /// One message from `from`: an ask is kept for the host to take, a
    /// guest's actions go straight to their client.
    pub fn receive(&mut self, session: &mut Session, from: PeerId, up: Up) -> Option<Event> {
        let guest = self.guests.iter_mut().find(|g| g.from == from).map(|g| {
            if matches!(up, Up::Actions(_)) {
                g.acked = g.acked.wrapping_add(1);
            }
            g.player
        });
        match (up, guest) {
            (
                Up::Ask {
                    name,
                    user_id,
                    template,
                },
                None,
            ) => {
                self.offers.retain(|o| o.from != from);
                self.offers.push(Offer {
                    from,
                    name: name.clone(),
                    user_id,
                    template,
                });
                Some(Event::Offered(name))
            }
            (Up::Actions(actions), Some(player)) => {
                let i = index_of(session, player)?;
                for action in actions.into_iter().filter(allowed) {
                    session.act(Peer::Client(i), action);
                }
                None
            }
            (Up::Purchase(bought), Some(player)) => Some(Event::Purchase { player, bought }),
            (Up::Leave, Some(player)) => {
                self.guests.retain(|g| g.player != player);
                if let Some(i) = index_of(session, player) {
                    session.leave(i);
                }
                Some(Event::Left(player))
            }
            (Up::Leave, None) => {
                self.offers.retain(|o| o.from != from);
                None
            }
            (Up::Resync, Some(_)) => {
                self.resync(Some(from));
                None
            }
            _ => None,
        }
    }

    /// Lets the ask from `from` in: they join the session wearing the
    /// avatar they sent. Their client's index and their welcome.
    pub fn accept(&mut self, session: &mut Session, from: PeerId) -> Result<(usize, Down), String> {
        let at = self
            .offers
            .iter()
            .position(|o| o.from == from)
            .ok_or("nobody asks to join from there")?;
        let offer = self.offers.remove(at);
        let template = match &offer.template {
            Some(bytes) => Some(rbx_binary::deserialize(bytes).map_err(|e| {
                format!(
                    "{}\u{2019}s avatar couldn\u{2019}t be read: {e}",
                    offer.name
                )
            })?),
            None => None,
        };
        let i = session.join(&offer.name, offer.user_id, template)?;
        let client = &mut session.clients[i];
        client.sim.watch_view_changes();
        let player = client.player;
        let name = client.name.clone();
        self.guests.push(Guest {
            from,
            player,
            encoder: Encoder::default(),
            started: false,
            offer,
            acked: 0,
            held: None,
        });
        Ok((i, Down::Welcome { player, name }))
    }

    /// Turns the ask from `from` down; the answer to send them.
    pub fn decline(&mut self, from: PeerId) -> Down {
        self.offers.retain(|o| o.from != from);
        Down::Ended("The host didn\u{2019}t let you in.".into())
    }

    /// `frame` for `player`'s editor, if they're remote: the ops for the
    /// shot's changes (the whole tree the first time) filled in, its sound
    /// events, the input applied, the core GUI dropped when it didn't
    /// change. `slow` (the link is behind): held back and merged into the
    /// next one sent.
    pub fn frame(
        &mut self,
        player: Ref,
        shot: Shot<'_>,
        mut frame: Frame,
        slow: bool,
    ) -> Option<(PeerId, Down)> {
        let guest = self.guests.iter_mut().find(|g| g.player == player)?;
        frame.reset = !guest.started;
        frame.ops = if guest.started {
            guest.encoder.ops(shot.dom, shot.changes)
        } else {
            guest.encoder.snapshot(shot.dom)
        };
        guest.started = true;
        frame.core = frame.core.and_then(|core| guest.encoder.core(core));
        frame.sounds = guest.encoder.sounds(shot.dom, &shot.sounds, shot.time);
        frame.ack = guest.acked;
        let frame = match guest.held.take() {
            Some(held) => held.merge(frame),
            None => frame,
        };
        if slow {
            guest.held = Some(frame);
            return None;
        }
        Some((guest.from, Down::Frame(Box::new(frame))))
    }

    /// Takes every remote player out of the session, to carry them into
    /// the one a teleport starts ([`Host::arrive`]) rather than end them.
    pub fn carry(&mut self) -> Vec<Offer> {
        self.guests.drain(..).map(|g| g.offer).collect()
    }

    /// Joins the players [`Host::carry`] took into `session`, wearing the
    /// avatars they came with: for each, their new welcome (their first
    /// frame there is the whole tree), or why they couldn't come.
    pub fn arrive(&mut self, session: &mut Session, carried: Vec<Offer>) -> Vec<(PeerId, Down)> {
        carried
            .into_iter()
            .map(|offer| {
                let from = offer.from;
                self.offers.retain(|o| o.from != from);
                self.offers.push(offer);
                let down = match self.accept(session, from) {
                    Ok((_, welcome)) => welcome,
                    Err(why) => Down::Ended(why),
                };
                (from, down)
            })
            .collect()
    }

    /// The next frame to `from` (`None`: to every guest, after the host's
    /// own connection dropped) is the whole tree again.
    pub fn resync(&mut self, from: Option<PeerId>) {
        for guest in &mut self.guests {
            if from.is_none_or(|f| f == guest.from) {
                guest.started = false;
            }
        }
    }

    /// `player` is gone (left, kicked, their tab closed): whom to tell.
    pub fn drop_player(&mut self, player: Ref) -> Option<PeerId> {
        let at = self.guests.iter().position(|g| g.player == player)?;
        Some(self.guests.remove(at).from)
    }

    /// The session ended: everyone waiting or playing, to tell.
    pub fn end(&mut self) -> Vec<PeerId> {
        let mut out: Vec<PeerId> = self.guests.drain(..).map(|g| g.from).collect();
        out.extend(self.offers.drain(..).map(|o| o.from));
        out
    }
}

#[cfg(test)]
mod tests;
