//! The frame loop: scripts and physics stepped together.

use std::cell::{Ref as CellRef, RefCell};
use std::rc::Rc;

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_lua::server::{Event, ServerVm};
use rbx_lua::Output;
use rbx_reflection::ReflectionDatabase;

use crate::physics::{Bridge, Shared, Touch, World, PHYSICS_HZ};
use crate::scripts;

/// Frames a second, Roblox's server rate.
pub const FRAME_HZ: f64 = 60.0;

const DEFAULT_GRAVITY: f64 = 196.2;
const DEFAULT_FALLEN_HEIGHT: f64 = -500.0;

/// Property writes that change what physics is built from.
const STRUCTURAL: &[&str] = &[
    "Size",
    "size",
    "Shape",
    "shape",
    "Anchored",
    "CanCollide",
    "CanTouch",
    "Massless",
    "Material",
    "CustomPhysicalProperties",
    "CollisionGroup",
    "CollisionGroupId",
    "Part0",
    "Part1",
    "Part0Internal",
    "Part1Internal",
    "Enabled",
];

/// A place running headless.
pub struct Simulation {
    dom: Rc<RefCell<WeakDom>>,
    db: &'static ReflectionDatabase,
    vm: ServerVm,
    world: World,
    frame: u64,
    dirty: bool,
    output: Output,
}

impl Simulation {
    pub fn new(dom: WeakDom) -> Result<Self, String> {
        let db = ReflectionDatabase::shared();
        let dom = Rc::new(RefCell::new(dom));
        dom.borrow_mut().take_changes();
        let shared = Rc::new(RefCell::new(Shared::default()));
        let vm = ServerVm::new(dom.clone(), db.clone(), Rc::new(Bridge(shared.clone())))
            .map_err(|err| err.to_string())?;
        Ok(Simulation {
            dom,
            db,
            vm,
            world: World::new(shared),
            frame: 0,
            dirty: true,
            output: Output::default(),
        })
    }

    /// Starts every script the server would run, in tree order, and runs
    /// their bodies up to their first yield, at time zero.
    pub fn start(&mut self) {
        let roots = self.dom.borrow().root_refs().to_vec();
        for root in roots {
            self.start_scripts_under(root);
        }
        self.rebuild();
        let changes = self.vm.resume();
        self.apply(changes);
        self.collect_output();
    }

    pub fn dom(&self) -> CellRef<'_, WeakDom> {
        self.dom.borrow()
    }

    /// Game time, seconds since the Run started.
    pub fn time(&self) -> f64 {
        self.frame as f64 / FRAME_HZ
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// What scripts printed since the last call.
    pub fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }

    /// Runs frames until `seconds` of game time have passed.
    pub fn run_for(&mut self, seconds: f64) {
        let frames = (seconds * FRAME_HZ).round() as u64;
        for _ in 0..frames {
            self.step();
        }
    }

    /// One frame, in the order a real server runs it (the RunServiceOrder
    /// golden): `PreAnimation`, `Stepped`, `PreSimulation`, physics,
    /// `PostSimulation`, the threads whose wait is over (`task.wait`,
    /// `task.delay`), `Heartbeat`, the step's `Touched`/`TouchEnded`
    /// (after `Heartbeat`: the TouchedOrder golden), the deferred queue.
    pub fn step(&mut self) {
        let dt = 1.0 / FRAME_HZ;
        let started = self.time();
        self.frame += 1;
        let now = self.time();
        self.vm.set_time(now);

        self.fire(Event::RunService("PreAnimation", vec![dt]));
        self.fire(Event::RunService("Stepped", vec![started, dt]));
        self.fire(Event::RunService("PreSimulation", vec![dt]));

        let touches = self.physics_step();

        self.fire(Event::RunService("PostSimulation", vec![dt]));
        self.resume();
        self.fire(Event::RunService("Heartbeat", vec![dt]));
        self.fire_touches(touches);
        self.resume();
        self.collect_output();
    }

    fn resume(&mut self) {
        let changes = self.vm.resume();
        self.apply(changes);
    }

    fn physics_step(&mut self) -> Vec<Touch> {
        if self.dirty {
            self.rebuild();
        }
        let (gravity, fallen_height) = self.workspace_numbers();
        let steps = (PHYSICS_HZ / FRAME_HZ).round() as u32;
        let touches = self.world.step(gravity, steps, self.adaptive_stepping());
        self.world.write_back(&mut self.dom.borrow_mut(), self.db);
        for part in self.world.fallen(fallen_height) {
            let changes = self.vm.destroy(part);
            self.apply(changes);
        }
        touches
    }

    /// `Touched`/`TouchEnded` for both parts of each touch.
    fn fire_touches(&mut self, touches: Vec<Touch>) {
        for touch in touches {
            for (part, other) in [(touch.a, touch.b), (touch.b, touch.a)] {
                self.fire(if touch.began {
                    Event::Touched { part, other }
                } else {
                    Event::TouchEnded { part, other }
                });
            }
        }
    }

    fn fire(&mut self, event: Event) {
        let changes = self.vm.fire(event);
        self.apply(changes);
    }

    fn rebuild(&mut self) {
        let groups = self.vm.collision_groups();
        self.world
            .rebuild(&self.dom.borrow(), self.db, &groups.borrow());
        self.dirty = false;
    }

    /// `Workspace.PhysicsSteppingMethod`: only `Fixed` keeps everything
    /// at 240 Hz; `Default` stepped adaptively on the server we recorded.
    fn adaptive_stepping(&self) -> bool {
        const FIXED: u32 = 1;
        let dom = self.dom.borrow();
        let method = dom
            .root_refs()
            .iter()
            .filter_map(|r| dom.get(*r))
            .find(|i| i.class() == "Workspace")
            .and_then(|w| self.db.stored_or_default(w, "PhysicsSteppingMethod"))
            .map(|(_, v)| v.clone());
        !matches!(method, Some(Variant::Enum(FIXED)))
    }

    fn workspace_numbers(&self) -> (f64, f64) {
        let dom = self.dom.borrow();
        let workspace = dom
            .root_refs()
            .iter()
            .filter_map(|r| dom.get(*r))
            .find(|i| i.class() == "Workspace");
        let number = |name: &str, default: f64| {
            workspace
                .and_then(|w| self.db.stored_or_default(w, name))
                .and_then(|(_, v)| match v {
                    Variant::Float32(x) => Some(f64::from(*x)),
                    Variant::Float64(x) => Some(*x),
                    _ => None,
                })
                .unwrap_or(default)
        };
        (
            number("Gravity", DEFAULT_GRAVITY),
            number("FallenPartsDestroyHeight", DEFAULT_FALLEN_HEIGHT),
        )
    }

    fn start_scripts_under(&mut self, root: Ref) {
        let found: Vec<Ref> = {
            let dom = self.dom.borrow();
            scripts::subtree(&dom, root)
                .into_iter()
                .filter(|r| scripts::runs(&dom, *r))
                .collect()
        };
        for script in found {
            if let Err(err) = self.vm.start_script(script) {
                self.output.push_text(rbx_lua::LineKind::Error, err.to_string());
            }
        }
    }

    /// Feeds what scripts changed to physics: a moved part moves its body,
    /// a velocity write sets it, anything structural rebuilds the world
    /// before the next step. Scripts newly placed where they run start.
    fn apply(&mut self, changes: Vec<Change>) {
        for change in changes {
            match change {
                Change::Property { referent, name } => self.property_changed(referent, &name),
                Change::Added(referent) | Change::Parent { referent, .. } => {
                    if self.touches_physics(referent) {
                        self.dirty = true;
                    }
                    self.start_scripts_under(referent);
                }
                Change::Removed(referent) => {
                    if self.world.is_part(referent) || self.world.is_joint(referent) {
                        self.dirty = true;
                    }
                }
                Change::Class(_) => self.dirty = true,
            }
        }
    }

    fn property_changed(&mut self, referent: Ref, name: &str) {
        let dom = self.dom.borrow();
        let Some(instance) = dom.get(referent) else {
            return;
        };
        let class = instance.class();
        let is_part = self.db.is_subclass_of(class, "BasePart");
        let is_joint = self.db.is_subclass_of(class, "JointInstance") || class == "WeldConstraint";
        if !(is_part || is_joint) {
            if class == "Script" && name != "Source" {
                drop(dom);
                self.start_scripts_under(referent);
            }
            return;
        }
        if STRUCTURAL.contains(&name) || (is_part && !self.world.is_part(referent)) {
            self.dirty = true;
            return;
        }
        if !is_part {
            return;
        }
        let read = |n: &str| self.db.stored_or_default(instance, n).map(|(_, v)| v.clone());
        match name {
            "CFrame" => {
                if let Some(Variant::CFrame(cf)) = read("CFrame") {
                    drop(dom);
                    self.world.teleport(referent, &cf);
                }
            }
            "AssemblyLinearVelocity" | "Velocity" | "AssemblyAngularVelocity" | "RotVelocity" => {
                let angular = matches!(name, "AssemblyAngularVelocity" | "RotVelocity");
                if let Some(Variant::Vector3(v)) = instance.properties().get(name).cloned() {
                    drop(dom);
                    self.world.set_velocity(referent, v, angular);
                }
            }
            _ => {}
        }
    }

    fn touches_physics(&self, root: Ref) -> bool {
        let dom = self.dom.borrow();
        scripts::subtree(&dom, root).into_iter().any(|r| {
            dom.get(r).is_some_and(|i| {
                let class = i.class();
                self.db.is_subclass_of(class, "BasePart")
                    || self.db.is_subclass_of(class, "JointInstance")
                    || class == "WeldConstraint"
            })
        })
    }

    fn collect_output(&mut self) {
        self.output.extend(self.vm.take_output());
    }
}
