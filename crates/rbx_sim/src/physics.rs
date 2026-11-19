//! Rigid-body physics on Rapier: the workspace's parts, glued into
//! assemblies by their rigid joints, stepped at Roblox's 240 Hz.
//!
//! What stands in for what (see `docs/research/local-run/PLAN.md`):
//! - an assembly (parts joined by `WeldConstraint`, `Weld`, `Motor6D`,
//!   any other `JointInstance`) is one rigid body; a joint holds its parts
//!   where they stood when the world was built, `C0`/`C1` aren't re-solved;
//! - each part is one collider on that body: a box, a ball, a cylinder or
//!   a convex hull for the wedges; `MeshPart`, `TrussPart` and the CSG
//!   classes collide as their `Size` box;
//! - friction and elasticity of a contact are the pairwise weighted average
//!   the `PhysicalProperties` docs give, done in a contact hook.

pub(crate) mod part;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;

use rapier3d::prelude::*;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::server::{CollisionGroups, Physics, QueryFilter, QueryHit, QueryShape};
use rbx_reflection::ReflectionDatabase;

pub use part::Shape;
use part::{Joint, PartData};

use crate::spatial::Spatial;

/// Roblox's physics rate: four solver steps per 60 Hz frame.
pub const PHYSICS_HZ: f64 = 240.0;

/// Friction and elasticity of one part with their weights, for the
/// contact hook (indexed by collider user data).
#[derive(Clone, Copy, Debug)]
struct Surface {
    friction: f32,
    elasticity: f32,
    friction_weight: f32,
    elasticity_weight: f32,
}

struct Placed {
    assembly: usize,
    /// The part's pose in its body's frame.
    offset: Pose,
    can_touch: bool,
}

struct Assembly {
    body: RigidBodyHandle,
    parts: Vec<Ref>,
    anchored: bool,
}

/// What scripts can ask of physics between steps (`Physics` for the VM):
/// masses as of the last build, impulses for the next step.
#[derive(Default)]
pub struct Shared {
    masses: HashMap<Ref, (f64, f64)>,
    impulses: Vec<(Ref, [f64; 3], bool)>,
    /// Bumped whenever the world steps or is rebuilt.
    generation: u64,
    queries: Option<Queries>,
}

/// What spatial queries read: the live DOM, the groups, the index.
struct Queries {
    dom: Rc<RefCell<WeakDom>>,
    db: &'static ReflectionDatabase,
    groups: Rc<RefCell<CollisionGroups>>,
    spatial: Rc<RefCell<Spatial>>,
}

impl Shared {
    /// Wires the spatial queries to the DOM and the VM's collision groups.
    pub fn connect(
        &mut self,
        dom: Rc<RefCell<WeakDom>>,
        db: &'static ReflectionDatabase,
        groups: Rc<RefCell<CollisionGroups>>,
    ) {
        self.queries = Some(Queries {
            dom,
            db,
            groups,
            spatial: Rc::new(RefCell::new(Spatial::default())),
        });
    }
}

/// The VM's view of [`Shared`].
pub struct Bridge(pub Rc<RefCell<Shared>>);

impl Bridge {
    fn query<R>(&self, f: impl FnOnce(&Spatial, &CollisionGroups) -> R) -> Option<R> {
        let (dom, db, groups, spatial, generation) = {
            let shared = self.0.borrow();
            let q = shared.queries.as_ref()?;
            (
                q.dom.clone(),
                q.db,
                q.groups.clone(),
                q.spatial.clone(),
                shared.generation,
            )
        };
        let dom = dom.borrow();
        let mut spatial = spatial.borrow_mut();
        spatial.refresh(&dom, db, generation);
        let groups = groups.borrow();
        Some(f(&spatial, &groups))
    }
}

impl Physics for Bridge {
    fn raycast(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        filter: &QueryFilter,
    ) -> Option<QueryHit> {
        self.query(|s, g| s.raycast(origin, direction, filter, g))?
    }

    fn shapecast(
        &self,
        shape: &QueryShape,
        from: &CFrameData,
        direction: [f64; 3],
        filter: &QueryFilter,
    ) -> Option<QueryHit> {
        self.query(|s, g| s.shapecast(shape, from, direction, filter, g))?
    }

    fn overlap(
        &self,
        shape: &QueryShape,
        at: &CFrameData,
        exact: bool,
        filter: &QueryFilter,
    ) -> Vec<Ref> {
        self.query(|s, g| s.overlap(shape, at, exact, filter, g))
            .unwrap_or_default()
    }

    fn mass(&self, part: Ref) -> Option<f64> {
        self.0.borrow().masses.get(&part).map(|m| m.0)
    }

    fn assembly_mass(&self, part: Ref) -> Option<f64> {
        self.0.borrow().masses.get(&part).map(|m| m.1)
    }

    fn apply_impulse(&self, part: Ref, impulse: [f64; 3], angular: bool) {
        self.0.borrow_mut().impulses.push((part, impulse, angular));
    }
}

/// A touch that began or ended during a step, between two parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch {
    pub a: Ref,
    pub b: Ref,
    pub began: bool,
}

#[derive(Default)]
struct Events(Mutex<Vec<CollisionEvent>>);

impl EventHandler for Events {
    fn handle_collision_event(
        &self,
        _: &RigidBodySet,
        _: &ColliderSet,
        event: CollisionEvent,
        _: Option<&ContactPair>,
    ) {
        if let Ok(mut list) = self.0.lock() {
            list.push(event);
        }
    }

    fn handle_contact_force_event(
        &self,
        _: Real,
        _: &RigidBodySet,
        _: &ColliderSet,
        _: &ContactPair,
        _: Real,
    ) {
    }

    fn handle_soft_body_tear_event(&self, _: &SoftBodySet, _: &SoftBodyTearEvent) {}
}

struct Mixer<'a>(&'a [Surface]);

impl PhysicsHooks for Mixer<'_> {
    fn modify_solver_contacts(&self, context: &mut ContactModificationContext) {
        let surface = |handle: ColliderHandle| {
            context
                .colliders
                .get(handle)
                .and_then(|c| self.0.get(c.user_data as usize))
                .copied()
        };
        let (Some(a), Some(b)) = (surface(context.collider1), surface(context.collider2)) else {
            return;
        };
        let mix = |x: f32, wx: f32, y: f32, wy: f32| {
            if wx + wy > 0.0 {
                (x * wx + y * wy) / (wx + wy)
            } else {
                (x + y) / 2.0
            }
        };
        if let Some(rigid) = context.rigid_mut() {
            *rigid.friction = mix(a.friction, a.friction_weight, b.friction, b.friction_weight);
            *rigid.restitution = mix(
                a.elasticity,
                a.elasticity_weight,
                b.elasticity,
                b.elasticity_weight,
            );
        }
    }
}

pub struct World {
    pipeline: PhysicsPipeline,
    params: IntegrationParameters,
    islands: IslandManager,
    broad: BroadPhaseBvh,
    narrow: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft: SoftBodySet,
    ccd: CCDSolver,
    surfaces: Vec<Surface>,
    parts: HashMap<Ref, Placed>,
    by_collider: HashMap<ColliderHandle, Ref>,
    assemblies: Vec<Assembly>,
    /// Joints and parts the last build saw, so a change can tell whether
    /// it touches physics.
    joints: HashSet<Ref>,
    shared: Rc<RefCell<Shared>>,
    /// Built since the last step, so no contact is known yet.
    fresh: bool,
    events: Events,
}

impl World {
    pub fn new(shared: Rc<RefCell<Shared>>) -> Self {
        let params = IntegrationParameters {
            dt: (1.0 / PHYSICS_HZ) as Real,
            ..Default::default()
        };
        World {
            pipeline: PhysicsPipeline::new(),
            params,
            islands: IslandManager::new(),
            broad: BroadPhaseBvh::new(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            surfaces: Vec::new(),
            parts: HashMap::new(),
            by_collider: HashMap::new(),
            assemblies: Vec::new(),
            joints: HashSet::new(),
            shared,
            fresh: true,
            events: Events::default(),
        }
    }

    pub fn is_part(&self, part: Ref) -> bool {
        self.parts.contains_key(&part)
    }

    pub fn is_joint(&self, joint: Ref) -> bool {
        self.joints.contains(&joint)
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    pub fn assembly_count(&self) -> usize {
        self.assemblies.len()
    }

    /// Throws the world away and builds it again from the workspace in
    /// `dom`: parts, assemblies, velocities as the DOM holds them.
    pub fn rebuild(&mut self, dom: &WeakDom, db: &ReflectionDatabase, groups: &CollisionGroups) {
        *self = World::new(self.shared.clone());
        self.shared.borrow_mut().generation += 1;
        let Some(workspace) = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        else {
            return;
        };
        let mut parts = Vec::new();
        let mut joints = Vec::new();
        let mut stack = vec![workspace];
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().rev());
            if let Some(part) = PartData::read(db, instance) {
                parts.push(part);
            } else if let Some(joint) = Joint::read(db, instance) {
                joints.push(joint);
            }
        }
        let index: HashMap<Ref, usize> = parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.referent, i))
            .collect();
        let mut sets = UnionFind::new(parts.len());
        for joint in &joints {
            self.joints.insert(joint.referent);
            if let (Some(&a), Some(&b)) = (index.get(&joint.part0), index.get(&joint.part1)) {
                sets.union(a, b);
            }
        }
        let mut groups_of: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..parts.len() {
            groups_of.entry(sets.find(i)).or_default().push(i);
        }
        let mut members: Vec<Vec<usize>> = groups_of.into_values().collect();
        members.sort_by_key(|m| m[0]);
        for member in members {
            self.add_assembly(&parts, &member, groups);
        }
        self.publish_masses(&parts);
    }

    fn add_assembly(&mut self, parts: &[PartData], member: &[usize], groups: &CollisionGroups) {
        let anchored = member.iter().any(|&i| parts[i].anchored);
        let root = root_part(parts, member);
        let root_pose = pose_of(&parts[root].cframe);
        let builder = if anchored {
            RigidBodyBuilder::fixed()
        } else {
            RigidBodyBuilder::dynamic()
                .linvel(vec3(parts[root].linear_velocity))
                .angvel(vec3(parts[root].angular_velocity))
        };
        let body = self.bodies.insert(builder.pose(root_pose).build());
        let assembly = self.assemblies.len();
        for &i in member {
            let part = &parts[i];
            let offset = root_pose.inverse() * pose_of(&part.cframe);
            let surface = self.surfaces.len();
            self.surfaces.push(Surface {
                friction: part.physics[1],
                elasticity: part.physics[2],
                friction_weight: part.physics[3],
                elasticity_weight: part.physics[4],
            });
            // A massless part adds no mass, unless it is the root (docs).
            let density = if part.massless && i != root {
                0.0
            } else {
                part.physics[0]
            };
            let group = groups.index(&part.collision_group);
            let filter = groups
                .names()
                .iter()
                .enumerate()
                .filter(|(_, other)| groups.collides(&part.collision_group, other))
                .fold(0u32, |bits, (j, _)| bits | (1 << j));
            let collider = part
                .shape_collider()
                .position(offset)
                .density(density)
                .sensor(!part.can_collide)
                .user_data(surface as u128)
                .active_hooks(ActiveHooks::MODIFY_SOLVER_CONTACTS)
                .active_events(if part.can_touch {
                    ActiveEvents::COLLISION_EVENTS
                } else {
                    ActiveEvents::empty()
                })
                .collision_groups(InteractionGroups::new(
                    Group::from_bits_truncate(1 << group),
                    Group::from_bits_truncate(filter),
                    InteractionTestMode::And,
                ))
                .build();
            let handle = self
                .colliders
                .insert_with_parent(collider, body, &mut self.bodies);
            self.by_collider.insert(handle, part.referent);
            self.parts.insert(
                part.referent,
                Placed {
                    assembly,
                    offset,
                    can_touch: part.can_touch,
                },
            );
        }
        self.assemblies.push(Assembly {
            body,
            parts: member.iter().map(|&i| parts[i].referent).collect(),
            anchored,
        });
    }

    fn publish_masses(&self, parts: &[PartData]) {
        let mut shared = self.shared.borrow_mut();
        shared.masses.clear();
        for part in parts {
            let Some(placed) = self.parts.get(&part.referent) else {
                continue;
            };
            let own = part.volume() * f64::from(part.physics[0]);
            let assembly = &self.assemblies[placed.assembly];
            let total = if assembly.anchored {
                f64::INFINITY
            } else {
                self.bodies
                    .get(assembly.body)
                    .map_or(own, |b| f64::from(b.mass()))
            };
            shared.masses.insert(part.referent, (own, total));
        }
    }

    /// Moves `part`'s whole assembly so that `part` lands on `cframe`, as a
    /// script's `CFrame`/`Position` write does.
    pub fn teleport(&mut self, part: Ref, cframe: &CFrameData) {
        let Some(placed) = self.parts.get(&part) else {
            return;
        };
        let pose = pose_of(cframe) * placed.offset.inverse();
        if let Some(body) = self.bodies.get_mut(self.assemblies[placed.assembly].body) {
            body.set_position(pose, true);
        }
    }

    /// A script's velocity write.
    pub fn set_velocity(&mut self, part: Ref, velocity: Vector3Data, angular: bool) {
        let Some(placed) = self.parts.get(&part) else {
            return;
        };
        if let Some(body) = self.bodies.get_mut(self.assemblies[placed.assembly].body) {
            if angular {
                body.set_angvel(vec3(velocity), true);
            } else {
                body.set_linvel(vec3(velocity), true);
            }
        }
    }

    /// Whether any of `assembly`'s parts is in contact with something.
    fn in_contact(&self, assembly: &Assembly) -> bool {
        let Some(body) = self.bodies.get(assembly.body) else {
            return false;
        };
        body.colliders().iter().any(|&collider| {
            self.narrow
                .contact_pairs_with(collider)
                .any(|pair| pair.has_any_active_contact())
        })
    }

    /// One frame of physics: `steps` solver steps of 1/240 s under
    /// `gravity` (studs/s², down), with the impulses scripts queued applied
    /// first. Returns the touches that began or ended.
    ///
    /// `adaptive` is Roblox's adaptive timestepping, which puts "easy"
    /// assemblies on a 60 Hz island and hard ones on 240 Hz (its docs keep
    /// the criteria to themselves). Ours: an assembly touching nothing
    /// moves in one 60 Hz step a frame (gravity for the whole frame up
    /// front, then the four substeps coast), anything in contact at 240 Hz.
    /// The `FreeFall` golden is what this matches.
    pub fn step(&mut self, gravity: f64, steps: u32, adaptive: bool) -> Vec<Touch> {
        self.shared.borrow_mut().generation += 1;
        // A world just built has no contacts yet: everything takes 240 Hz once.
        let adaptive = adaptive && !std::mem::replace(&mut self.fresh, false);
        let frame = Vector::new(0.0, -gravity as Real, 0.0) * (steps as Real * self.params.dt);
        let free: Vec<(RigidBodyHandle, bool)> = self
            .assemblies
            .iter()
            .filter(|a| !a.anchored)
            .map(|a| (a.body, adaptive && !self.in_contact(a)))
            .collect();
        for (handle, free) in free {
            if let Some(body) = self.bodies.get_mut(handle) {
                body.set_gravity_scale(if free { 0.0 } else { 1.0 }, false);
                if free && !body.is_sleeping() {
                    body.set_linvel(body.linvel() + frame, false);
                }
            }
        }
        let impulses = std::mem::take(&mut self.shared.borrow_mut().impulses);
        for (part, impulse, angular) in impulses {
            let Some(placed) = self.parts.get(&part) else {
                continue;
            };
            if let Some(body) = self.bodies.get_mut(self.assemblies[placed.assembly].body) {
                let impulse =
                    Vector::new(impulse[0] as Real, impulse[1] as Real, impulse[2] as Real);
                if angular {
                    body.apply_torque_impulse(impulse, true);
                } else {
                    body.apply_impulse(impulse, true);
                }
            }
        }
        let gravity = Vector::new(0.0, -gravity as Real, 0.0);
        let mixer = Mixer(&self.surfaces);
        for _ in 0..steps {
            self.pipeline.step(
                gravity,
                &self.params,
                &mut self.islands,
                &mut self.broad,
                &mut self.narrow,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                &mut self.soft,
                &mut self.ccd,
                &mixer,
                &self.events,
            );
        }
        let events = self
            .events
            .0
            .lock()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default();
        events
            .into_iter()
            .filter_map(|event| {
                let (c1, c2, began) = match event {
                    CollisionEvent::Started(a, b, _) => (a, b, true),
                    CollisionEvent::Stopped(a, b, _) => (a, b, false),
                };
                let (a, b) = (*self.by_collider.get(&c1)?, *self.by_collider.get(&c2)?);
                let touchable = |r: Ref| self.parts.get(&r).is_some_and(|p| p.can_touch);
                (touchable(a) && touchable(b)).then_some(Touch { a, b, began })
            })
            .collect()
    }

    /// Writes every moving part's `CFrame` and its assembly's velocities
    /// into `dom`, unlogged: Roblox fires no `Changed` for what physics
    /// moves.
    pub fn write_back(&self, dom: &mut WeakDom, db: &ReflectionDatabase) {
        for assembly in &self.assemblies {
            if assembly.anchored {
                continue;
            }
            let Some(body) = self.bodies.get(assembly.body) else {
                continue;
            };
            let pose = *body.position();
            let linear = vector_data(body.linvel());
            let angular = vector_data(body.angvel());
            for part in &assembly.parts {
                let Some(placed) = self.parts.get(part) else {
                    continue;
                };
                let Some(instance) = dom.get_mut(*part) else {
                    continue;
                };
                let class = instance.class().to_owned();
                let props = instance.properties_mut();
                props.insert(
                    "CFrame".into(),
                    Variant::CFrame(cframe_of(&(pose * placed.offset))),
                );
                for (name, value) in [
                    ("AssemblyLinearVelocity", linear),
                    ("AssemblyAngularVelocity", angular),
                ] {
                    let key = db
                        .stored_names(&class, name)
                        .first()
                        .copied()
                        .unwrap_or(name)
                        .to_owned();
                    props.insert(key, Variant::Vector3(value));
                }
            }
        }
    }

    /// Every part physics moves (the unanchored assemblies' parts, awake or
    /// not): what [`World::write_back`] writes `CFrame` for.
    pub fn moving_parts(&self) -> impl Iterator<Item = Ref> + '_ {
        self.assemblies
            .iter()
            .filter(|a| !a.anchored)
            .flat_map(|a| a.parts.iter().copied())
    }

    /// The parts of every unanchored assembly whose position is below
    /// `height` (`Workspace.FallenPartsDestroyHeight`).
    pub fn fallen(&self, height: f64) -> Vec<Ref> {
        let mut out = Vec::new();
        for assembly in self.assemblies.iter().filter(|a| !a.anchored) {
            let Some(body) = self.bodies.get(assembly.body) else {
                continue;
            };
            for part in &assembly.parts {
                let Some(placed) = self.parts.get(part) else {
                    continue;
                };
                let y = (*body.position() * placed.offset).translation.y;
                if f64::from(y) < height {
                    out.push(*part);
                }
            }
        }
        out
    }
}

/// The part an assembly's body is built at: the biggest by volume, the
/// first on a tie (Roblox picks by `RootPriority`, then mass; close enough
/// for a body origin, which changes nothing physical).
fn root_part(parts: &[PartData], member: &[usize]) -> usize {
    member
        .iter()
        .copied()
        .max_by(|&a, &b| {
            parts[a]
                .volume()
                .total_cmp(&parts[b].volume())
                .then(b.cmp(&a))
        })
        .unwrap_or(member[0])
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind((0..n).collect())
    }

    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.0[root] != root {
            root = self.0[root];
        }
        let mut at = i;
        while self.0[at] != root {
            let next = self.0[at];
            self.0[at] = root;
            at = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

fn vec3(v: Vector3Data) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

fn vector_data(v: Vector) -> Vector3Data {
    Vector3Data {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

/// `CFrameData`'s rotation is row-major.
pub(crate) fn pose_of(cframe: &CFrameData) -> Pose {
    let r = cframe.rotation;
    let matrix = Mat3::from_cols(
        Vector::new(r[0], r[3], r[6]),
        Vector::new(r[1], r[4], r[7]),
        Vector::new(r[2], r[5], r[8]),
    );
    Pose::from_parts(
        vec3(cframe.position),
        Rotation::from_mat3(&matrix).normalize(),
    )
}

pub(crate) fn cframe_of(pose: &Pose) -> CFrameData {
    let m = Mat3::from_quat(pose.rotation);
    let (x, y, z) = (m.x_axis, m.y_axis, m.z_axis);
    CFrameData {
        position: vector_data(pose.translation),
        rotation: [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z],
    }
}
