//! Humanoids: the state machine, health and death, `Move`/`MoveTo`, jumps,
//! climbing, seats, and the drive handed to physics every frame
//! (`physics::Drive`). Clean room: written from the Humanoid docs and the
//! Humanoid goldens recorded on a real server; numbers are cited where
//! they're used.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rbx_dom::{CFrameData, Change, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::physics::{Drive, DriveMode, World};

/// Floor sensor reach past the standing height: the frames Landed comes on
/// after 3, 5 and 8 stud drops (HumanoidSensor `d3`, `d5`, `d8`, five
/// passes) bound it to 0.99 to 1.2 studs.
const SENSOR_REACH: f64 = 1.1;
/// Frames without a floor before Running turns to Freefall (HumanoidDrop and
/// HumanoidSensor: `Running>Freefall@7`, the ledge 7 frames after the floor
/// went).
const FREEFALL_FRAMES: u32 = 7;
/// Frames a character stays Landed (HumanoidDrop `Landed>Running` 2 after).
const LANDED_FRAMES: u32 = 2;
/// `MoveTo` gives up after 8 s (HumanoidMoveTo `timeout`: 480 beats).
const MOVE_TO_TIMEOUT: u64 = 480;
/// A `MoveTo` counts as there once the root, where the last frame left it,
/// is this close (HumanoidMoveTo `near_5`, `near_12`, `near_20` and the
/// 0.87 stud short `arrive_dx` bound it to 1.01 to 1.07).
const MOVE_TO_REACHED: f64 = 1.04;
/// JumpPower 30, 50, 70, 100 take off at 1.06 times the power
/// (HumanoidJumpScale); JumpHeight h at sqrt(2 g h).
const JUMP_POWER_SCALE: f64 = 1.06;
/// Climbing speed per WalkSpeed: 11.2 stud/s at 16 (HumanoidClimb).
const CLIMB_PER_WALKSPEED: f64 = 0.7;
/// How far from a truss an R15 holds while climbing (HumanoidClimb
/// `r15_gap` 1.96 = 1 + 0.5 + 0.46); an R6 holds against it.
const CLIMB_GAP_R15: f64 = 0.46;
/// A truss this close past the holding gap starts a climb, and the rig
/// walks in to the gap as it climbs (HumanoidClimb `_r15_x`, `_r6_x`: an
/// R15 starts 0.15 stud past it, not 0.42; an R6 only once it touches).
const CLIMB_REACH_R15: f64 = 0.25;
const CLIMB_REACH_R6: f64 = 0.1;
/// Swim speed per WalkSpeed (Swim `vx*`: 11.8 and 13.5 stud/s at 16).
const SWIM_PER_WALKSPEED_R15: f64 = 0.74;
const SWIM_PER_WALKSPEED_R6: f64 = 0.875;
/// How fast an R15 swimmer's bob dies out, per second (Swim `_r15_sink`
/// settles in a second where an R6 one, left to the water, bobs for two).
const SWIM_DAMPING_R15: f64 = 4.0;

/// `Enum.HumanoidStateType`, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    FallingDown,
    Ragdoll,
    GettingUp,
    Jumping,
    Swimming,
    Freefall,
    Flying,
    Landed,
    Running,
    RunningNoPhysics,
    StrafingNoPhysics,
    Climbing,
    Seated,
    PlatformStanding,
    Dead,
    Physics,
    None,
}

const STATES: [State; 17] = [
    State::FallingDown,
    State::Ragdoll,
    State::GettingUp,
    State::Jumping,
    State::Swimming,
    State::Freefall,
    State::Flying,
    State::Landed,
    State::Running,
    State::RunningNoPhysics,
    State::StrafingNoPhysics,
    State::Climbing,
    State::Seated,
    State::PlatformStanding,
    State::Dead,
    State::Physics,
    State::None,
];

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::FallingDown => "FallingDown",
            State::Ragdoll => "Ragdoll",
            State::GettingUp => "GettingUp",
            State::Jumping => "Jumping",
            State::Swimming => "Swimming",
            State::Freefall => "Freefall",
            State::Flying => "Flying",
            State::Landed => "Landed",
            State::Running => "Running",
            State::RunningNoPhysics => "RunningNoPhysics",
            State::StrafingNoPhysics => "StrafingNoPhysics",
            State::Climbing => "Climbing",
            State::Seated => "Seated",
            State::PlatformStanding => "PlatformStanding",
            State::Dead => "Dead",
            State::Physics => "Physics",
            State::None => "None",
        }
    }

    fn from_name(name: &str) -> Option<State> {
        STATES.iter().copied().find(|s| s.name() == name)
    }

    fn index(self) -> usize {
        STATES.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn arg(self) -> Arg {
        Arg::Enum {
            enum_name: "HumanoidStateType".into(),
            item: self.name().into(),
        }
    }

    /// States the character walks in.
    fn walks(self) -> bool {
        matches!(
            self,
            State::Running
                | State::RunningNoPhysics
                | State::Landed
                | State::Jumping
                | State::Freefall
        )
    }

    fn grounded(self) -> bool {
        matches!(
            self,
            State::Running | State::RunningNoPhysics | State::Landed
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rig {
    R6,
    R15,
}

#[derive(Clone, Debug)]
struct WalkTo {
    point: [f64; 3],
    part: Option<Ref>,
    started: u64,
}

/// Frames after leaving a seat before touching one sits again.
const SEAT_COOLDOWN: u64 = 60;
const SEAT_JUMP_LIFT: f64 = 0.945;

/// One humanoid the simulation runs.
#[derive(Clone, Debug)]
pub struct Character {
    pub humanoid: Ref,
    pub model: Ref,
    pub root: Ref,
    pub state: State,
    rig: Rig,
    /// Frames since the state was entered, counted at the end of each
    /// frame's update.
    age: u32,
    no_floor: u32,
    fresh: bool,
    yaw: f64,
    /// How fast AutoRotate is turning it, rad/s.
    spin: f64,
    move_dir: [f64; 3],
    /// The floor's velocity under the root, zero with no floor.
    floor_velocity: [f64; 3],
    walk_to: Option<WalkTo>,
    running: f64,
    had_neck: bool,
    disabled: u32,
    stand: f64,
    /// The `HipHeight` `stand` was worked out for: a change moves the
    /// standing height with it (HumanoidPosture `_up`, `_down`).
    hip: f64,
    half: [f64; 3],
    /// A jump's take-off speed and how far the root is lifted first.
    launch: Option<[f64; 2]>,
    /// How far to walk in to a truss this frame while climbing.
    approach: f64,
    /// The frame it died on.
    pub died_at: Option<u64>,
    /// The frame it last left a seat: a seat it is still touching doesn't
    /// take it back straight away (HumanoidSeat `jump_sit`).
    left_seat: Option<u64>,
}

/// Every humanoid in the place, by referent, and what's waiting to fire.
#[derive(Default)]
pub struct Humanoids {
    pub characters: BTreeMap<Ref, Character>,
    /// Every `Humanoid` in the DOM, kept up from its edits.
    found: BTreeSet<Ref>,
    journal: crate::journal::Journal,
    health_seen: HashMap<Ref, f64>,
    pub events: Vec<Event>,
    /// Instances to destroy (broken joints), in order.
    pub destroy: Vec<Ref>,
    pub frame: u64,
}

/// `found` after `changes`: a humanoid comes with its `Added` and goes with
/// its `Removed`, wherever it sits.
fn track(found: &mut BTreeSet<Ref>, dom: &WeakDom, changes: &[Change]) {
    for change in changes {
        match change {
            Change::Added(r) | Change::Class(r) => {
                if dom.get(*r).is_some_and(|i| i.class() == "Humanoid") {
                    found.insert(*r);
                } else {
                    found.remove(r);
                }
            }
            Change::Removed(r) => {
                found.remove(r);
            }
            _ => {}
        }
    }
}

fn signal(owner: Ref, name: &str, args: Vec<Arg>) -> Event {
    Event::Signal {
        owner,
        name: name.into(),
        args,
    }
}

pub(crate) fn number(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> f64 {
    let Some(instance) = dom.get(r) else {
        return 0.0;
    };
    match db.stored_or_default(instance, name).map(|(_, v)| v) {
        Some(Variant::Float32(v)) => f64::from(*v),
        Some(Variant::Float64(v)) => *v,
        Some(Variant::Int32(v)) => f64::from(*v),
        Some(Variant::Int64(v)) => *v as f64,
        _ => 0.0,
    }
}

pub(crate) fn flag(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> bool {
    dom.get(r)
        .and_then(|i| db.stored_or_default(i, name))
        .is_some_and(|(_, v)| matches!(v, Variant::Bool(true)))
}

fn enum_value(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> u32 {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::Enum(v))) => *v,
        _ => 0,
    }
}

pub(crate) fn reference(dom: &WeakDom, r: Ref, name: &str) -> Option<Ref> {
    match dom.get(r)?.properties().get(name) {
        Some(Variant::Ref(v)) if dom.get(*v).is_some() => Some(*v),
        _ => None,
    }
}

pub(crate) fn vector(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> [f64; 3] {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::Vector3(v))) => [f64::from(v.x), f64::from(v.y), f64::from(v.z)],
        _ => [0.0; 3],
    }
}

pub(crate) fn cframe(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> Option<CFrameData> {
    match dom.get(r).and_then(|i| db.stored_or_default(i, "CFrame")) {
        Some((_, Variant::CFrame(c))) => Some(*c),
        _ => None,
    }
}

/// `Humanoid:Move(v, true)`: `v` in the current camera's frame, its -Z the
/// camera's look direction laid flat (its up direction when it looks
/// straight down).
fn relative_to_camera(dom: &WeakDom, v: [f64; 3]) -> [f64; 3] {
    let Some((camera, _)) = crate::input::camera(dom) else {
        return v;
    };
    let r = camera.rotation.map(f64::from);
    let flat = |x: f64, z: f64| {
        let length = (x * x + z * z).sqrt();
        (length > 1e-4).then(|| [x / length, z / length])
    };
    let Some(ahead) = flat(-r[2], -r[8]).or_else(|| flat(r[1], r[7])) else {
        return v;
    };
    // Right of "ahead", on the ground.
    let right = [-ahead[1], ahead[0]];
    [
        right[0] * v[0] - ahead[0] * v[2],
        0.0,
        right[1] * v[0] - ahead[1] * v[2],
    ]
}

/// Writes `name` under the name the DOM stores it as (logged, so scripts
/// see `Changed` and the simulation routes it).
pub(crate) fn set(dom: &mut WeakDom, db: &ReflectionDatabase, r: Ref, name: &str, value: Variant) {
    let Some(instance) = dom.get(r) else { return };
    let key = match db.stored_or_default(instance, name) {
        Some((key, old)) => {
            let value = match (old, &value) {
                (Variant::Float32(_), Variant::Float64(v)) => Variant::Float32(*v as f32),
                _ => value.clone(),
            };
            let key = key.to_owned();
            let _ = dom.set_property(r, &key, value);
            return;
        }
        None => db.stored_names(instance.class(), name)[0].to_owned(),
    };
    let _ = dom.set_property(r, &key, value);
}

fn vec3(v: [f64; 3]) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v[0] as f32,
        y: v[1] as f32,
        z: v[2] as f32,
    })
}

fn position(c: &CFrameData) -> [f64; 3] {
    [
        f64::from(c.position.x),
        f64::from(c.position.y),
        f64::from(c.position.z),
    ]
}

/// The turn about +Y a CFrame faces (0 faces -Z, its LookVector).
fn yaw_of(c: &CFrameData) -> f64 {
    let r = c.rotation;
    let look = [-f64::from(r[2]), -f64::from(r[8])];
    (-look[0]).atan2(-look[1])
}

fn children_named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

fn descendants(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        if let Some(i) = dom.get(at) {
            stack.extend(i.children().iter().rev());
            if at != root {
                out.push(at);
            }
        }
    }
    out
}

fn is_joint(db: &ReflectionDatabase, class: &str) -> bool {
    db.is_subclass_of(class, "JointInstance") || class == "WeldConstraint"
}

fn has_neck(dom: &WeakDom, db: &ReflectionDatabase, model: Ref) -> bool {
    descendants(dom, model).into_iter().any(|r| {
        dom.get(r).is_some_and(|i| {
            i.name() == "Neck"
                && is_joint(db, i.class())
                && reference(dom, r, "Part0").is_some()
                && reference(dom, r, "Part1").is_some()
        })
    })
}

/// The parts of a rig that keep colliding (HumanoidStand `*_collide`): the
/// rest are turned off when the humanoid takes it over.
/// `BodyPartsCollidableOnDeath` of the first `AvatarCollisionRules` in the
/// game, true when there is none (the class default, read live).
fn collidable_on_death(dom: &WeakDom, db: &ReflectionDatabase) -> bool {
    let mut stack = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(i) = dom.get(r) else { continue };
        if i.class() == "AvatarCollisionRules" {
            return flag(dom, db, r, "BodyPartsCollidableOnDeath");
        }
        stack.extend(i.children().iter().copied());
    }
    true
}

fn keeps_colliding(rig: Rig, name: &str) -> bool {
    match rig {
        Rig::R15 => matches!(
            name,
            "Head" | "HumanoidRootPart" | "UpperTorso" | "LowerTorso"
        ),
        Rig::R6 => matches!(name, "Head" | "HumanoidRootPart" | "Torso"),
    }
}

/// Whether `humanoid` sits in a model in Workspace with a root part: its
/// model and root.
fn rig_of(dom: &WeakDom, db: &ReflectionDatabase, humanoid: Ref) -> Option<(Ref, Ref)> {
    let model = dom.parent(humanoid)?;
    if !db.is_subclass_of(dom.get(model)?.class(), "Model") || !in_workspace(dom, model) {
        return None;
    }
    let root = children_named(dom, model, "HumanoidRootPart")
        .or_else(|| reference(dom, model, "PrimaryPart"))
        .filter(|r| {
            dom.get(*r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
        })?;
    Some((model, root))
}

fn in_workspace(dom: &WeakDom, mut at: Ref) -> bool {
    while let Some(parent) = dom.parent(at) {
        at = parent;
    }
    dom.get(at).is_some_and(|i| i.class() == "Workspace")
}

impl Humanoids {
    /// Starts running every humanoid newly in Workspace with a rig, stops
    /// those that left. A new one is Running at once (HumanoidStand
    /// `*_state0`) and says so a beat later (`RunningNoPhysics>Running@0`).
    pub fn sync(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase) {
        let log = self.journal.log(dom);
        track(&mut self.found, dom, log);
        if !self.journal.reset(dom) {
            self.found.clear();
            let mut stack: Vec<Ref> = dom.root_refs().to_vec();
            while let Some(at) = stack.pop() {
                let Some(i) = dom.get(at) else { continue };
                stack.extend(i.children().iter().copied());
                if i.class() == "Humanoid" {
                    self.found.insert(at);
                }
            }
        }
        let found = &self.found;
        self.characters.retain(|h, c| {
            found.contains(h) && rig_of(dom, db, *h).is_some_and(|(_, r)| r == c.root)
        });
        for humanoid in self.found.clone() {
            self.register(dom, db, humanoid);
        }
    }

    /// Takes note of `changes`, drained from the DOM's log (numbered from
    /// `first`): the humanoids that came and went.
    pub(crate) fn note(&mut self, dom: &WeakDom, first: u64, changes: &[Change]) {
        let fresh = self.journal.fresh(first, changes);
        track(&mut self.found, dom, fresh);
    }

    /// Takes `humanoid` over if it has a rig and isn't run yet.
    pub fn register(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, humanoid: Ref) -> bool {
        if self.characters.contains_key(&humanoid) {
            return true;
        }
        let Some((model, root)) = rig_of(dom, db, humanoid) else {
            return false;
        };
        let rig = if enum_value(dom, db, humanoid, "RigType") == 1 {
            Rig::R15
        } else {
            Rig::R6
        };
        let size = vector(dom, db, root, "Size");
        let root_cf = cframe(dom, db, root).unwrap_or(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        });
        let hip = number(dom, db, humanoid, "HipHeight");
        let mut stand = hip + size[1] / 2.0;
        if rig == Rig::R6 {
            let feet = ["Left Leg", "Right Leg"]
                .iter()
                .filter_map(|n| children_named(dom, model, n))
                .filter_map(|leg| {
                    let c = cframe(dom, db, leg)?;
                    Some(f64::from(c.position.y) - vector(dom, db, leg, "Size")[1] / 2.0)
                })
                .fold(f64::INFINITY, f64::min);
            if feet.is_finite() {
                stand = f64::from(root_cf.position.y) - feet + hip;
            }
        }
        let parts: Vec<Ref> = dom
            .get(model)
            .map(|m| m.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|c| {
                dom.get(*c).is_some_and(|i| {
                    db.is_subclass_of(i.class(), "BasePart") && !keeps_colliding(rig, i.name())
                })
            })
            .collect();
        for part in parts {
            set(dom, db, part, "CanCollide", Variant::Bool(false));
        }
        let had_neck = has_neck(dom, db, model);
        self.characters.insert(
            humanoid,
            Character {
                humanoid,
                model,
                root,
                state: State::Running,
                rig,
                age: 0,
                no_floor: 0,
                fresh: true,
                yaw: yaw_of(&root_cf),
                spin: 0.0,
                move_dir: [0.0; 3],
                floor_velocity: [0.0; 3],
                walk_to: None,
                running: 0.0,
                had_neck,
                disabled: 0,
                stand,
                hip,
                half: [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0],
                launch: None,
                approach: 0.0,
                died_at: None,
                left_seat: None,
            },
        );
        self.health_seen
            .insert(humanoid, number(dom, db, humanoid, "Health"));
        self.events.push(signal(
            humanoid,
            "StateChanged",
            vec![State::RunningNoPhysics.arg(), State::Running.arg()],
        ));
        true
    }

    /// A state another peer's humanoid went into (state replication):
    /// `StateChanged` and the state's own signals fire here too.
    /// It arrives at the start of the next frame, which is when it counts
    /// here (a respawn waits from then: PlayersCharacter `respawn`).
    pub fn replicate_state(&mut self, humanoid: Ref, to: State) {
        let frame = self.frame + 1;
        if to == State::Dead {
            if let Some(c) = self.characters.get_mut(&humanoid) {
                c.died_at.get_or_insert(frame);
            }
        }
        self.change(humanoid, to);
    }

    fn change(&mut self, humanoid: Ref, to: State) {
        let Some(c) = self.characters.get_mut(&humanoid) else {
            return;
        };
        if c.state == to {
            return;
        }
        let from = std::mem::replace(&mut c.state, to);
        c.age = 0;
        c.no_floor = 0;
        if to == State::Swimming {
            // `Swimming` fires on the way in, at rest too (Swim `idle_speed`).
            c.running = -1.0;
        }
        self.events
            .push(signal(humanoid, "StateChanged", vec![from.arg(), to.arg()]));
        let state_event = |name: &str, on: bool| signal(humanoid, name, vec![Arg::Bool(on)]);
        for (state, name) in [
            (State::Jumping, "Jumping"),
            (State::Freefall, "FreeFalling"),
            (State::PlatformStanding, "PlatformStanding"),
            (State::GettingUp, "GettingUp"),
            (State::FallingDown, "FallingDown"),
            (State::Ragdoll, "Ragdoll"),
        ] {
            if from == state {
                self.events.push(state_event(name, false));
            }
            if to == state {
                self.events.push(state_event(name, true));
            }
        }
    }

    /// `Health` or `MaxHealth` was just written: `HealthChanged` when the
    /// health moved, and on every MaxHealth write (HumanoidHealth: three
    /// `health:100` for `Health = 200`, `MaxHealth = 50`, `MaxHealth = 100`).
    pub fn health_written(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        humanoid: Ref,
        name: &str,
    ) {
        let health = number(dom, db, humanoid, "Health");
        self.health_changed(humanoid, name, health);
    }

    /// The same for a script's write about to be stored, `health` being
    /// what `Health` will read: the event carries the value of the moment
    /// even when the script writes again before it yields.
    pub fn health_changed(&mut self, humanoid: Ref, name: &str, health: f64) {
        let seen = self.health_seen.insert(humanoid, health);
        if name == "MaxHealth" || seen != Some(health) {
            self.events
                .push(signal(humanoid, "HealthChanged", vec![Arg::Number(health)]));
        }
    }

    /// Everything that happens before physics in a frame, in order: death,
    /// `MoveTo` steering, property-driven states, the state machine's
    /// floor checks (on where the last frame left the root), jumps,
    /// climbing; then each character's drive.
    pub(crate) fn before_physics(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        world: &World,
        terrain: &crate::terrain::Terrain,
        gravity: f64,
    ) -> Vec<Drive> {
        let ids: Vec<Ref> = self.characters.keys().copied().collect();
        let mut drives = Vec::new();
        for id in ids {
            self.health(dom, db, id);
            // Another peer simulates it: its state comes from there.
            if self
                .characters
                .get(&id)
                .is_some_and(|c| world.is_remote(c.root))
            {
                continue;
            }
            self.arrive(dom, db, world, id);
            self.steer(dom, db, id);
            self.properties(dom, db, id);
            self.machine(dom, db, world, terrain, id, gravity);
            let Some(c) = self.characters.get_mut(&id) else {
                continue;
            };
            c.age = c.age.saturating_add(1);
            c.fresh = false;
            drives.push(self.drive(dom, db, id));
        }
        drives
    }

    fn health(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, id: Ref) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let (model, state, had_neck) = (c.model, c.state, c.had_neck);
        if state == State::Dead {
            // A dead humanoid's Health goes back to 0 (HumanoidHealth
            // `health:50@91 health:0@91`).
            if number(dom, db, id, "Health") > 0.0 {
                set(dom, db, id, "Health", Variant::Float64(0.0));
                self.health_written(dom, db, id, "Health");
            }
            return;
        }
        // In the Physics state the humanoid runs nothing, the neck check
        // included: a ragdoll that takes its Motor6Ds out lives (seen live
        // in a real game that does exactly that).
        if had_neck
            && state != State::Physics
            && flag(dom, db, id, "RequiresNeck")
            && !has_neck(dom, db, model)
        {
            set(dom, db, id, "Health", Variant::Float64(0.0));
        }
        if number(dom, db, id, "Health") > 0.0 {
            return;
        }
        // Health is pulled up to 0, then Died, then the Dead state
        // (HumanoidHealth `health:-890 health:0 died state:Dead`).
        set(dom, db, id, "Health", Variant::Float64(0.0));
        self.health_written(dom, db, id, "Health");
        self.events.push(signal(id, "Died", Vec::new()));
        self.change(id, State::Dead);
        let frame = self.frame;
        if let Some(c) = self.characters.get_mut(&id) {
            c.died_at = Some(frame);
        }
        // The limbs collide again once dead (`AvatarCollisionRules.
        // BodyPartsCollidableOnDeath`, true by default; HumanoidDeath: every
        // part reads CanCollide true after Died, and an unbroken body
        // stands on its feet).
        if collidable_on_death(dom, db) {
            let rig = self.characters.get(&id).map(|c| c.rig);
            for part in dom
                .get(model)
                .map(|m| m.children().to_vec())
                .unwrap_or_default()
            {
                let limb = dom.get(part).is_some_and(|i| {
                    db.is_subclass_of(i.class(), "BasePart")
                        && rig.is_some_and(|rig| !keeps_colliding(rig, i.name()))
                });
                if limb {
                    set(dom, db, part, "CanCollide", Variant::Bool(true));
                }
            }
        }
        if flag(dom, db, id, "BreakJointsOnDeath") {
            for r in descendants(dom, model) {
                if dom.get(r).is_some_and(|i| is_joint(db, i.class())) {
                    self.destroy.push(r);
                }
            }
        }
    }

    fn steer(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, id: Ref) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let Some(to) = c.walk_to.clone() else { return };
        let root = cframe(dom, db, c.root)
            .map(|c| position(&c))
            .unwrap_or_default();
        let target = target_of(dom, db, &to);
        let flat = [target[0] - root[0], 0.0, target[2] - root[2]];
        let length = (flat[0] * flat[0] + flat[2] * flat[2]).sqrt();
        if length > 1e-9 {
            let dir = [flat[0] / length, 0.0, flat[2] / length];
            self.set_move(dom, db, id, dir);
        }
    }

    fn set_move(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, id: Ref, dir: [f64; 3]) {
        if let Some(c) = self.characters.get_mut(&id) {
            c.move_dir = dir;
        }
        set(dom, db, id, "MoveDirection", vec3(dir));
    }

    fn properties(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, id: Ref) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let state = c.state;
        if state == State::Dead {
            return;
        }
        let platform = flag(dom, db, id, "PlatformStand");
        if platform && state != State::PlatformStanding {
            self.change(id, State::PlatformStanding);
        } else if !platform && state == State::PlatformStanding {
            self.change(id, State::Running);
        }
        let sit = flag(dom, db, id, "Sit");
        if sit && state != State::Seated && self.enabled(id, State::Seated) {
            self.change(id, State::Seated);
        }
    }

    fn enabled(&self, id: Ref, state: State) -> bool {
        self.characters
            .get(&id)
            .is_some_and(|c| c.disabled & (1 << state.index()) == 0)
    }

    fn machine(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        world: &World,
        terrain: &crate::terrain::Terrain,
        id: Ref,
        gravity: f64,
    ) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let (state, age, root, stand, half, yaw, fresh) =
            (c.state, c.age, c.root, c.stand, c.half, c.yaw, c.fresh);
        let floor = world.floor_under(root, stand, [half[0], half[2]], yaw, SENSOR_REACH);
        let vy = world.part_velocity(root).map_or(0.0, |v| v[1]);
        // `FloorMaterial`: the floor's material, Air with none.
        const AIR: u32 = 1792;
        let material = floor.map_or(AIR, |f| {
            f.terrain_material
                .unwrap_or_else(|| enum_value(dom, db, f.part, "Material"))
        });
        set(dom, db, id, "FloorMaterial", Variant::Enum(material));
        if let Some(c) = self.characters.get_mut(&id) {
            c.floor_velocity = floor.map_or([0.0; 3], |f| f.velocity);
        }
        // In water once the root is under the surface (Swim: in at beat 13
        // of a drop from 2 above it).
        let water = if terrain.has_water() {
            world
                .part_pose(root)
                .map_or(0.0, |p| terrain.water_at(p.translation))
        } else {
            0.0
        };
        // Bobbing up out of it is still swimming (Swim `_r6_sink`).
        let (wet, in_water) = (water >= 0.5, water > 0.0);
        match state {
            State::Freefall if wet && vy <= 0.0 && self.enabled(id, State::Swimming) => {
                self.change(id, State::Swimming)
            }
            // Feet found the bottom: up and running (Swim `shelf_state`).
            State::Swimming if floor.is_some() => self.change(id, State::GettingUp),
            State::Swimming if !in_water => self.change(id, State::Freefall),
            State::Jumping if age >= 1 => self.change(id, State::Freefall),
            State::Freefall if age >= 1 && floor.is_some() && vy <= 0.0 => {
                self.change(id, State::Landed)
            }
            State::Landed if age >= LANDED_FRAMES => self.change(id, State::Running),
            State::GettingUp if age >= 1 => self.change(id, State::Running),
            State::Ragdoll if age >= 1 => self.change(id, State::GettingUp),
            State::Running | State::RunningNoPhysics if floor.is_none() => {
                if let Some(c) = self.characters.get_mut(&id) {
                    if !fresh {
                        c.no_floor += 1;
                    }
                    if c.no_floor >= FREEFALL_FRAMES {
                        self.change(id, State::Freefall);
                    }
                }
            }
            State::Running | State::RunningNoPhysics => {
                if let Some(c) = self.characters.get_mut(&id) {
                    c.no_floor = 0;
                }
            }
            _ => {}
        }
        self.climb_check(dom, db, world, id);
        self.jump_check(dom, db, id, gravity);
    }

    fn climb_check(&mut self, dom: &WeakDom, db: &ReflectionDatabase, world: &World, id: Ref) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let (state, root, half, yaw, rig, move_dir) =
            (c.state, c.root, c.half, c.yaw, c.rig, c.move_dir);
        if !(state.grounded() || state == State::Climbing || state == State::Freefall)
            || !self.enabled(id, State::Climbing)
        {
            return;
        }
        let (gap, reach) = match rig {
            Rig::R15 => (CLIMB_GAP_R15, CLIMB_REACH_R15),
            Rig::R6 => (0.0, CLIMB_REACH_R6),
        };
        let ahead = world
            .part_ahead(root, half[2], yaw, gap + reach)
            .filter(|(part, _)| dom.get(*part).is_some_and(|i| i.class() == "TrussPart"));
        let step = number(dom, db, id, "WalkSpeed") / 60.0;
        if let Some(c) = self.characters.get_mut(&id) {
            c.approach = ahead.map_or(0.0, |(_, d)| (d - gap).clamp(0.0, step));
        }
        let look = [-yaw.sin(), -yaw.cos()];
        let pushing = move_dir[0] * look[0] + move_dir[2] * look[1] > 0.1;
        match (state, ahead) {
            (State::Climbing, None) => self.change(id, State::Freefall),
            (State::Climbing, Some(_)) => {}
            (_, Some(_)) if pushing => {
                self.change(id, State::Climbing);
                self.events
                    .push(signal(id, "Climbing", vec![Arg::Number(0.0)]));
            }
            _ => {}
        }
    }

    fn jump_check(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, id: Ref, gravity: f64) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let state = c.state;
        if !flag(dom, db, id, "Jump") {
            return;
        }
        let can = matches!(
            state,
            State::Running
                | State::RunningNoPhysics
                | State::Landed
                | State::Climbing
                | State::Seated
                | State::Swimming
        );
        if !can || !self.enabled(id, State::Jumping) {
            return;
        }
        if state == State::Seated {
            self.unsit(dom, db, id);
        }
        let mut speed = if flag(dom, db, id, "UseJumpPower") {
            JUMP_POWER_SCALE * number(dom, db, id, "JumpPower")
        } else {
            (2.0 * gravity * number(dom, db, id, "JumpHeight").max(0.0)).sqrt()
        };
        // Off a seat the root is lifted first and leaves three substeps of
        // gravity slower (HumanoidSeat `_jump_y`: 0.945 stud, 50.55 stud/s
        // for JumpPower 50).
        let mut lift = 0.0;
        if state == State::Seated {
            speed -= 3.0 * gravity / 240.0;
            lift = SEAT_JUMP_LIFT;
        }
        if let Some(c) = self.characters.get_mut(&id) {
            c.launch = Some([speed, lift]);
        }
        set(dom, db, id, "Jump", Variant::Bool(false));
        self.change(id, State::Jumping);
    }

    fn drive(&mut self, dom: &WeakDom, db: &ReflectionDatabase, id: Ref) -> Drive {
        let c = self.characters.get_mut(&id).expect("character");
        let hip = number(dom, db, id, "HipHeight");
        c.stand += hip - c.hip;
        c.hip = hip;
        let mode = match c.state {
            s if s.grounded() => DriveMode::Ground,
            State::Jumping | State::Freefall => DriveMode::Air,
            State::Climbing => DriveMode::Climb,
            State::Swimming => DriveMode::Swim,
            State::Seated if reference(dom, id, "SeatPart").is_none() => DriveMode::Ground,
            _ => DriveMode::Off,
        };
        let speed = number(dom, db, id, "WalkSpeed");
        let walks = c.state.walks();
        let walk = if c.state == State::Swimming {
            let pace = speed
                * match c.rig {
                    Rig::R15 => SWIM_PER_WALKSPEED_R15,
                    Rig::R6 => SWIM_PER_WALKSPEED_R6,
                };
            [c.move_dir[0] * pace, 0.0, c.move_dir[2] * pace]
        } else if walks {
            [c.move_dir[0] * speed, 0.0, c.move_dir[2] * speed]
        } else {
            [0.0; 3]
        };
        let moving = c.move_dir[0].abs() + c.move_dir[2].abs() > 1e-6;
        let turn =
            ((walks || c.state == State::Swimming) && moving && flag(dom, db, id, "AutoRotate"))
                .then(|| (-c.move_dir[0]).atan2(-c.move_dir[2]));
        let look = [-c.yaw.sin(), -c.yaw.cos()];
        let up = c.move_dir[0] * look[0] + c.move_dir[2] * look[1];
        Drive {
            root: c.root,
            mode,
            stand: c.stand,
            half: [c.half[0], c.half[2]],
            walk,
            max_slope: number(dom, db, id, "MaxSlopeAngle").to_radians(),
            yaw: c.yaw,
            turn,
            spin: c.spin,
            launch: c.launch.take(),
            approach: c.approach,
            climb: CLIMB_PER_WALKSPEED * speed * up.max(0.0),
            swim_damping: match c.rig {
                Rig::R15 => SWIM_DAMPING_R15,
                Rig::R6 => 0.0,
            },
        }
    }

    /// After physics: facings back from the drives, `MoveTo` arrival and
    /// timeout, `Running` when the speed moved.
    pub fn after_physics(&mut self, world: &World, drives: &[Drive]) {
        for drive in drives {
            let Some(id) = self
                .characters
                .values()
                .find(|c| c.root == drive.root)
                .map(|c| c.humanoid)
            else {
                continue;
            };
            if drive.mode != DriveMode::Off {
                if let Some(c) = self.characters.get_mut(&id) {
                    c.yaw = drive.yaw;
                    c.spin = drive.spin;
                }
            }
            let v = world.part_velocity(drive.root).unwrap_or_default();
            let mut speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
            if speed < 0.1 {
                speed = 0.0;
            }
            let Some(c) = self.characters.get_mut(&id) else {
                continue;
            };
            let event = match c.state {
                State::Swimming => "Swimming",
                s if s.walks() => "Running",
                _ => continue,
            };
            if (speed - c.running).abs() > 0.01 {
                c.running = speed;
                self.events
                    .push(signal(id, event, vec![Arg::Number(speed)]));
            }
        }
    }

    fn arrive(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, world: &World, id: Ref) {
        let Some(c) = self.characters.get(&id) else {
            return;
        };
        let Some(to) = c.walk_to.clone() else { return };
        let root = world
            .part_pose(c.root)
            .map(|p| p.translation.to_array())
            .unwrap_or_default();
        let target = target_of(dom, db, &to);
        let distance = ((target[0] - root[0]).powi(2) + (target[2] - root[2]).powi(2)).sqrt();
        let reached = distance < MOVE_TO_REACHED;
        if reached || self.frame >= to.started + MOVE_TO_TIMEOUT {
            if let Some(c) = self.characters.get_mut(&id) {
                c.walk_to = None;
            }
            self.set_move(dom, db, id, [0.0; 3]);
            self.events
                .push(signal(id, "MoveToFinished", vec![Arg::Bool(reached)]));
        }
    }

    /// Seats `humanoid` on `seat`: a `SeatWeld` from the seat's top to the
    /// root (HumanoidSeat `weld`: C0 0.5 up on a 1 stud seat, C1 0.5 down,
    /// the root facing the seat's way), `Sit`, `SeatPart`, `Occupant`,
    /// Seated. The character is moved onto the seat first.
    pub fn sit(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        humanoid: Ref,
        seat: Ref,
    ) -> bool {
        if !self.register(dom, db, humanoid) {
            return false;
        }
        let c = self.characters[&humanoid].clone();
        if c.state == State::Dead || reference(dom, seat, "Occupant").is_some() {
            return false;
        }
        let Some(seat_cf) = cframe(dom, db, seat) else {
            return false;
        };
        let Some(root_cf) = cframe(dom, db, c.root) else {
            return false;
        };
        let up = (vector(dom, db, seat, "Size")[1] / 2.0) as f32;
        let c0 = translated(identity(), [0.0, up, 0.0]);
        let c1 = translated(identity(), [0.0, -0.5, 0.0]);
        let target = mul(&mul(&seat_cf, &c0), &inverse(&c1));
        let delta = mul(&target, &inverse(&root_cf));
        for part in descendants(dom, c.model) {
            if dom
                .get(part)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
            {
                if let Some(cf) = cframe(dom, db, part) {
                    set(dom, db, part, "CFrame", Variant::CFrame(mul(&delta, &cf)));
                }
            }
        }
        let weld = dom.new_instance("Weld", "SeatWeld", Some(seat));
        set(dom, db, weld, "Part0", Variant::Ref(seat));
        set(dom, db, weld, "Part1", Variant::Ref(c.root));
        set(dom, db, weld, "C0", Variant::CFrame(c0));
        set(dom, db, weld, "C1", Variant::CFrame(c1));
        set(dom, db, humanoid, "Sit", Variant::Bool(true));
        set(dom, db, humanoid, "SeatPart", Variant::Ref(seat));
        set(dom, db, seat, "Occupant", Variant::Ref(humanoid));
        self.change(humanoid, State::Seated);
        self.events.push(signal(
            humanoid,
            "Seated",
            vec![Arg::Bool(true), Arg::Instance(seat)],
        ));
        true
    }

    fn unsit(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, humanoid: Ref) {
        if let Some(seat) = reference(dom, humanoid, "SeatPart") {
            if let Some(weld) = children_named(dom, seat, "SeatWeld") {
                self.destroy.push(weld);
            }
            let _ = dom.remove_property(seat, "Occupant");
            self.events
                .push(signal(humanoid, "Seated", vec![Arg::Bool(false), Arg::Nil]));
        }
        let _ = dom.remove_property(humanoid, "SeatPart");
        set(dom, db, humanoid, "Sit", Variant::Bool(false));
        let frame = self.frame;
        if let Some(c) = self.characters.get_mut(&humanoid) {
            c.left_seat = Some(frame);
        }
    }

    /// A part touched another: a humanoid's part on an enabled, empty seat
    /// sits it there (HumanoidSeat `touch_sit`, `disabled_sit`).
    pub fn touched(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, a: Ref, b: Ref) {
        // `Humanoid.Touched(touchingPart, humanoidPart)` for a body part.
        for (mine, other) in [(a, b), (b, a)] {
            let model = dom.parent(mine);
            if let Some(c) = self.characters.values().find(|c| Some(c.model) == model) {
                let args = vec![Arg::Instance(other), Arg::Instance(mine)];
                self.events.push(signal(c.humanoid, "Touched", args));
            }
        }
        for (seat, other) in [(a, b), (b, a)] {
            let Some(class) = dom.get(seat).map(|i| i.class().to_owned()) else {
                continue;
            };
            if !matches!(class.as_str(), "Seat" | "VehicleSeat") || flag(dom, db, seat, "Disabled")
            {
                continue;
            }
            let Some(model) = dom.parent(other) else {
                continue;
            };
            let Some(c) = self.characters.values().find(|c| c.model == model) else {
                continue;
            };
            if matches!(c.state, State::Jumping | State::Dead | State::Seated)
                || c.left_seat.is_some_and(|f| self.frame < f + SEAT_COOLDOWN)
            {
                continue;
            }
            let humanoid = c.humanoid;
            self.sit(dom, db, humanoid, seat);
        }
    }

    /// A humanoid method a script called.
    pub fn call(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        humanoid: Ref,
        method: &str,
        args: &[Arg],
    ) -> Result<CallReply, String> {
        let state_arg = |i: usize| match args.get(i) {
            Some(Arg::Enum { item, .. }) => State::from_name(item).ok_or("bad state".to_owned()),
            _ => Err(format!("Argument {} missing or nil", i + 1)),
        };
        match method {
            "GetState" => {
                let state = if self.register(dom, db, humanoid) {
                    self.characters[&humanoid].state
                } else {
                    State::None
                };
                Ok(CallReply::value(state.arg()))
            }
            "ChangeState" => {
                let state = state_arg(0)?;
                self.register(dom, db, humanoid);
                if self.enabled(humanoid, state) {
                    self.change(humanoid, state);
                }
                Ok(self.take_reply())
            }
            "SetStateEnabled" => {
                let state = state_arg(0)?;
                let on = matches!(args.get(1), Some(Arg::Bool(true)));
                self.register(dom, db, humanoid);
                if let Some(c) = self.characters.get_mut(&humanoid) {
                    if on {
                        c.disabled &= !(1 << state.index());
                    } else {
                        c.disabled |= 1 << state.index();
                    }
                }
                Ok(CallReply::default())
            }
            "GetStateEnabled" => {
                let state = state_arg(0)?;
                let on = self
                    .characters
                    .get(&humanoid)
                    .is_none_or(|c| c.disabled & (1 << state.index()) == 0);
                Ok(CallReply::value(Arg::Bool(on)))
            }
            "Move" => {
                let v = match args.first() {
                    Some(Arg::Vector3(v)) => [f64::from(v[0]), 0.0, f64::from(v[2])],
                    _ => return Err("Argument 1 missing or nil".into()),
                };
                let v = match args.get(1) {
                    Some(Arg::Bool(true)) => relative_to_camera(dom, v),
                    _ => v,
                };
                let length = (v[0] * v[0] + v[2] * v[2]).sqrt();
                let dir = if length > 1.0 {
                    [v[0] / length, 0.0, v[2] / length]
                } else {
                    v
                };
                self.register(dom, db, humanoid);
                // A zero move leaves a `MoveTo` walking: the default
                // controls send one every frame (ClientReplication's
                // `client_speed` walks 16 with them running).
                if length < 1e-6
                    && self
                        .characters
                        .get(&humanoid)
                        .is_some_and(|c| c.walk_to.is_some())
                {
                    return Ok(self.take_reply());
                }
                if let Some(c) = self.characters.get_mut(&humanoid) {
                    c.walk_to = None;
                }
                self.set_move(dom, db, humanoid, dir);
                // `Running` with the speed it has when the move changes
                // (HumanoidWalk `running`: 16 first, then the slowdown).
                if let Some(c) = self.characters.get(&humanoid) {
                    let v = vector(dom, db, c.root, "AssemblyLinearVelocity");
                    let speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
                    self.events
                        .push(signal(humanoid, "Running", vec![Arg::Number(speed)]));
                }
                Ok(self.take_reply())
            }
            "MoveTo" => {
                let point = match args.first() {
                    Some(Arg::Vector3(v)) => [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])],
                    _ => return Err("Argument 1 missing or nil".into()),
                };
                let part = match args.get(1) {
                    Some(Arg::Instance(p)) => Some(*p),
                    _ => None,
                };
                let walk_to = WalkTo {
                    point,
                    part,
                    started: self.frame,
                };
                set(dom, db, humanoid, "WalkToPoint", vec3(point));
                if let Some(p) = part {
                    set(dom, db, humanoid, "WalkToPart", Variant::Ref(p));
                }
                self.register(dom, db, humanoid);
                if let Some(c) = self.characters.get_mut(&humanoid) {
                    c.walk_to = Some(walk_to);
                }
                Ok(CallReply::default())
            }
            // The walk it is asked for: `MoveDirection` at `WalkSpeed`.
            "GetMoveVelocity" => {
                let speed = number(dom, db, humanoid, "WalkSpeed");
                let dir = self
                    .characters
                    .get(&humanoid)
                    .map_or([0.0; 3], |c| c.move_dir);
                Ok(CallReply::value(vec_arg(dir.map(|d| d * speed))))
            }
            "GetRelativeVelocityAtFloor" => {
                let Some(c) = self.characters.get(&humanoid) else {
                    return Ok(CallReply::value(vec_arg([0.0; 3])));
                };
                let v = vector(dom, db, c.root, "AssemblyLinearVelocity");
                let f = c.floor_velocity;
                Ok(CallReply::value(vec_arg([
                    v[0] - f[0],
                    v[1] - f[1],
                    v[2] - f[2],
                ])))
            }
            // Not switched on in live servers yet (NewApis
            // `collide_on_death`).
            "GetCanCollideOnDeath" => {
                Err("Humanoid:GetCanCollideOnDeath is not yet enabled".into())
            }
            "TakeDamage" => {
                let amount = match args.first() {
                    Some(Arg::Number(n)) => *n,
                    _ => return Err("Argument 1 missing or nil".into()),
                };
                let shielded = dom.parent(humanoid).is_some_and(|model| {
                    dom.get(model).is_some_and(|m| {
                        m.children()
                            .iter()
                            .any(|c| dom.get(*c).is_some_and(|i| i.class() == "ForceField"))
                    })
                });
                if !shielded {
                    let health = number(dom, db, humanoid, "Health");
                    set(
                        dom,
                        db,
                        humanoid,
                        "Health",
                        Variant::Float64(health - amount),
                    );
                    self.health_written(dom, db, humanoid, "Health");
                }
                Ok(CallReply::default())
            }
            _ => Err(format!("{method} is not a valid member of Humanoid")),
        }
    }

    /// Events queued during a call, fired before the call returns.
    fn take_reply(&mut self) -> CallReply {
        let fire = std::mem::take(&mut self.events)
            .into_iter()
            .filter_map(|e| match e {
                Event::Signal { owner, name, args } => Some((owner, name, args)),
                _ => None,
            })
            .collect();
        CallReply {
            fire,
            ..CallReply::default()
        }
    }
}

fn target_of(dom: &WeakDom, db: &ReflectionDatabase, to: &WalkTo) -> [f64; 3] {
    match to.part.and_then(|p| cframe(dom, db, p)) {
        Some(c) => {
            let p = position(&c);
            [p[0] + to.point[0], p[1] + to.point[1], p[2] + to.point[2]]
        }
        None => to.point,
    }
}

pub(crate) fn identity() -> CFrameData {
    CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

pub(crate) fn translated(mut c: CFrameData, by: [f32; 3]) -> CFrameData {
    c.position.x += by[0];
    c.position.y += by[1];
    c.position.z += by[2];
    c
}

/// `a * b`, CFrames as Roblox composes them.
pub(crate) fn mul(a: &CFrameData, b: &CFrameData) -> CFrameData {
    let r = &a.rotation;
    let s = &b.rotation;
    let mut out = [0.0f32; 9];
    for i in 0..3 {
        for j in 0..3 {
            out[i * 3 + j] = (0..3).map(|k| r[i * 3 + k] * s[k * 3 + j]).sum();
        }
    }
    let p = &b.position;
    let at = |i: usize| r[i * 3] * p.x + r[i * 3 + 1] * p.y + r[i * 3 + 2] * p.z;
    CFrameData {
        position: Vector3Data {
            x: a.position.x + at(0),
            y: a.position.y + at(1),
            z: a.position.z + at(2),
        },
        rotation: out,
    }
}

pub(crate) fn inverse(c: &CFrameData) -> CFrameData {
    let r = &c.rotation;
    let t = [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]];
    let p = &c.position;
    let at = |i: usize| -(t[i * 3] * p.x + t[i * 3 + 1] * p.y + t[i * 3 + 2] * p.z);
    CFrameData {
        position: Vector3Data {
            x: at(0),
            y: at(1),
            z: at(2),
        },
        rotation: t,
    }
}

fn vec_arg(v: [f64; 3]) -> Arg {
    Arg::Vector3(v.map(|x| x as f32))
}
