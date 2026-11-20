//! Rigid-body physics on Rapier: the workspace's parts, glued into
//! assemblies by their rigid joints, stepped at Roblox's 240 Hz.
//!
//! What stands in for what (see `docs/research/local-run/PLAN.md`):
//! - an assembly (parts joined by `WeldConstraint`, `Weld`, `Motor6D`,
//!   any other `JointInstance`) is one rigid body; a joint holds its parts
//!   where they stood when the world was built, `C0`/`C1` aren't re-solved;
//! - each part is one collider on that body: a box, a ball, a cylinder or
//!   a convex hull for the wedges; a `MeshPart` per its `CollisionFidelity`
//!   (`mesh.rs`); `TrussPart` and the CSG classes collide as their `Size` box;
//! - friction and elasticity of a contact are the pairwise weighted average
//!   the `PhysicalProperties` docs give, done in a contact hook;
//! - constraints and movers are in `constraints.rs`.

mod character;
mod constraints;
pub(crate) mod mesh;
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

pub use character::{Drive, DriveMode, Floor};
pub(crate) use constraints::is_constraint;
pub use part::Shape;
use part::{Joint, PartData};

use crate::spatial::Spatial;

/// Roblox's physics rate: four solver steps per 60 Hz frame.
pub const PHYSICS_HZ: f64 = 240.0;

/// Spin lost per second, exponentially (the Damping golden: 10 rad/s
/// falls to 9.019 in 2 s). Roblox applies no linear damping.
const ANGULAR_DAMPING: Real = 0.052;

/// Approach speed (studs/s) above which a contact counts as an impact,
/// solved at one point so the bounce adds no spin (the BoxBounce golden).
const IMPACT_SPEED: Real = 1.5;
/// A free body meeting something faster than this takes three quarters of
/// its free frame's gravity kick before it bounces; a slower one settles
/// (TouchedOrder's clean passes bounce for bounce, SurfaceMix, BoxBounce,
/// RestingContact).
const KICKED_IMPACT: Real = 6.0;
const IMPACT_KICK: Real = 0.75;
const FRAME: Real = 1.0 / 60.0;

/// Friction and elasticity of one part with their weights, for the
/// contact hook (indexed by collider user data).
#[derive(Clone, Copy, Debug)]
struct Surface {
    friction: f32,
    elasticity: f32,
    friction_weight: f32,
    elasticity_weight: f32,
    /// An anchored part's `AssemblyLinearVelocity`: it stays put, but what
    /// touches it is carried as on a conveyor belt (Conveyor golden).
    conveyor: Vector,
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
    /// Another peer simulates it: this one moves it where it is told.
    remote: bool,
    /// Largest `Size` component of its parts: the adaptive thresholds scale
    /// with it.
    size: Real,
    /// Substeps per gravity kick while free: 4 (60 Hz), 2 (120), 1 (240).
    rate: std::cell::Cell<u32>,
    /// Free (adaptive) on the last frame: it went through whatever it met.
    was_free: std::cell::Cell<bool>,
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
    /// The humanoids the simulation runs (scripts call into them).
    pub(crate) humanoids: crate::humanoid::Humanoids,
    /// The Players service's local player and characters.
    pub(crate) players: crate::players::Players,
    /// Animators and their tracks.
    pub(crate) animations: crate::animation::Animations,
    /// Who simulates what.
    pub(crate) ownership: crate::ownership::Ownership,
    /// Engine calls on a humanoid another peer simulates, for the session
    /// to hand to its owner: (humanoid, method, arguments).
    pub(crate) forwarded: Vec<(Ref, String, Vec<rbx_lua::server::Arg>)>,
    /// A client's GUI: layout, focus, the core GUI switches.
    pub(crate) gui: crate::gui::Gui,
    pub(crate) tools: crate::tools::Tools,
    pub(crate) teams: crate::teams::Teams,
    /// `ParticleEmitter:Emit`/`Clear` calls not yet handed to a renderer.
    pub(crate) bursts: Vec<(Ref, crate::Burst)>,
    pub(crate) sounds: crate::sound::Sounds,
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

    /// The DOM and the reflection database, once connected.
    pub(crate) fn dom_and_db(&self) -> Option<(Rc<RefCell<WeakDom>>, &'static ReflectionDatabase)> {
        self.queries.as_ref().map(|q| (q.dom.clone(), q.db))
    }

    /// The mass of a part the world hasn't built yet (made this frame):
    /// its volume times its density, read from the DOM.
    fn unbuilt_mass(&self, part: Ref) -> Option<f64> {
        let queries = self.queries.as_ref()?;
        let dom = queries.dom.try_borrow().ok()?;
        let data = PartData::read(queries.db, dom.get(part)?)?;
        Some(data.volume() * f64::from(data.physics[0]))
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

    fn register_mesh(&self, part: Ref, vertices: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) {
        mesh::register(part, vertices, triangles);
    }

    fn mass(&self, part: Ref) -> Option<f64> {
        let shared = self.0.borrow();
        shared
            .masses
            .get(&part)
            .map(|m| m.0)
            .or_else(|| shared.unbuilt_mass(part))
    }

    /// From the DOM's joints as they are now, so a weld made this frame
    /// counts at once (DensityMass `weld_assembly`); a massless part
    /// leaves it to the last build, which knows the root.
    fn assembly_mass(&self, part: Ref) -> Option<f64> {
        let shared = self.0.borrow();
        let built = shared.masses.get(&part).map(|m| m.1);
        let fresh = shared.queries.as_ref().and_then(|queries| {
            let dom = queries.dom.try_borrow().ok()?;
            let (parts, _) = crate::ownership::assembly_in_dom(&dom, queries.db, part);
            parts.iter().try_fold(0.0, |sum, &p| {
                let data = PartData::read(queries.db, dom.get(p)?)?;
                (!data.massless).then(|| sum + data.volume() * f64::from(data.physics[0]))
            })
        });
        fresh.or(built).or_else(|| shared.unbuilt_mass(part))
    }

    fn apply_impulse(&self, part: Ref, impulse: [f64; 3], angular: bool) {
        self.0.borrow_mut().impulses.push((part, impulse, angular));
    }

    fn engine_method(&self, class: &str, key: &str) -> bool {
        crate::engine::is_method(class, key)
    }

    fn engine_signal(&self, class: &str, key: &str) -> bool {
        crate::engine::is_signal(class, key)
    }

    fn call(
        &self,
        referent: Ref,
        class: &str,
        method: &str,
        args: Vec<rbx_lua::server::Arg>,
    ) -> Result<rbx_lua::server::CallReply, String> {
        crate::engine::call(&self.0, referent, class, method, args)
    }

    fn engine_property(
        &self,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Option<rbx_lua::server::Arg> {
        crate::engine::property(&self.0, referent, class, key)
    }

    fn signal_method(&self, class: &str, key: &str) -> bool {
        crate::animation::is_signal_method(class, key)
    }

    fn engine_write(
        &self,
        referent: Ref,
        class: &str,
        key: &str,
        value: &rbx_lua::server::Arg,
    ) -> Option<rbx_lua::server::Arg> {
        crate::engine::write(&self.0, referent, class, key, value)
    }

    fn network_owner(&self, part: Ref) -> Result<Option<Ref>, String> {
        let (_, anchored) = self.assembly(part)?;
        if anchored {
            return Err(crate::ownership::ANCHORED.into());
        }
        Ok(self.0.borrow().ownership.owner(part))
    }

    fn set_network_owner(&self, part: Ref, owner: Option<Ref>, auto: bool) -> Result<(), String> {
        let (parts, anchored) = self.assembly(part)?;
        if anchored {
            return Err(crate::ownership::ANCHORED.into());
        }
        self.0.borrow_mut().ownership.pin(&parts, owner, auto);
        Ok(())
    }

    fn network_ownership_auto(&self, part: Ref) -> Result<bool, String> {
        let (_, anchored) = self.assembly(part)?;
        if anchored {
            return Err(crate::ownership::ANCHORED.into());
        }
        Ok(self.0.borrow().ownership.is_auto(part))
    }

    fn reparented(
        &self,
        child: Ref,
        old: Option<Ref>,
        new: Option<Ref>,
        after: bool,
    ) -> Vec<(Ref, String, Vec<rbx_lua::server::Arg>)> {
        let Some((dom, db)) = self.0.borrow().dom_and_db() else {
            return Vec::new();
        };
        let Ok(mut dom) = dom.try_borrow_mut() else {
            return Vec::new();
        };
        let mut shared = self.0.borrow_mut();
        let server = !shared.players.client;
        let local = shared.players.local;
        let mut fire = shared
            .tools
            .reparented(&mut dom, db, server, local, child, old, new, after);
        if after && server && dom.get(child).is_some_and(|i| i.class() == "Team") {
            fire.extend(shared.teams.sync(&mut dom, db));
        }
        fire
    }

    fn property_written(
        &self,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Vec<(Ref, String, Vec<rbx_lua::server::Arg>)> {
        let Some((dom, db)) = self.0.borrow().dom_and_db() else {
            return Vec::new();
        };
        let Ok(mut dom) = dom.try_borrow_mut() else {
            return Vec::new();
        };
        let mut shared = self.0.borrow_mut();
        if shared.players.client {
            return Vec::new();
        }
        shared.teams.written(&mut dom, db, referent, class, key)
    }
}

impl Bridge {
    /// `part`'s assembly as the DOM has it now; the ownership API is
    /// server-only.
    fn assembly(&self, part: Ref) -> Result<(Vec<Ref>, bool), String> {
        let shared = self.0.borrow();
        if shared.ownership.client {
            return Err("Network Ownership API can only be called from the server".into());
        }
        let (dom, db) = shared.dom_and_db().ok_or("not running")?;
        drop(shared);
        let dom = dom.borrow();
        Ok(crate::ownership::assembly_in_dom(&dom, db, part))
    }
}

/// The two parts a rigid joint holds together.
pub(crate) fn joint_parts(
    db: &ReflectionDatabase,
    instance: &rbx_dom::Instance,
) -> Option<(Ref, Ref)> {
    Joint::read(db, instance).map(|j| (j.part0, j.part1))
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

/// The contact hook: surfaces by collider, and the bodies that are free
/// (on adaptive stepping) this frame.
struct Mixer<'a>(&'a [Surface], &'a HashSet<RigidBodyHandle>);

impl PhysicsHooks for Mixer<'_> {
    fn modify_solver_contacts(&self, context: &mut ContactModificationContext) {
        // A free body moves its whole frame through what it meets; the
        // contact is found after, and resolved the next frame (TouchedOrder
        // `_y`: the block ends its last falling frame 0.9 stud into the
        // floor, then bounces from the surface at half the speed it had).
        // The contacts stay, inert, so the touch is still seen this frame.
        let free = |b: Option<RigidBodyHandle>| b.is_some_and(|b| self.1.contains(&b));
        if free(context.rigid_body1) || free(context.rigid_body2) {
            if let Some(rigid) = context.rigid_mut() {
                *rigid.friction = 0.0;
                *rigid.restitution = 0.0;
                for contact in rigid.solver_contacts.iter_mut() {
                    contact.dist = 1.0e3;
                }
            }
            return;
        }
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
        // Two weights of 0 give 0 (SurfaceMix `f_both0` slides on).
        let mix = |x: f32, wx: f32, y: f32, wy: f32| {
            if wx + wy > 0.0 {
                (x * wx + y * wy) / (wx + wy)
            } else {
                0.0
            }
        };
        let velocity = |handle: Option<RigidBodyHandle>, at: Vector| {
            handle
                .and_then(|h| context.bodies.get(h))
                .map_or(Vector::ZERO, |b| b.velocity_at_point(at))
        };
        let (body1, body2) = (context.rigid_body1, context.rigid_body2);
        let Some(rigid) = context.rigid_mut() else {
            return;
        };
        let belt = a.conveyor - b.conveyor;
        if belt != Vector::ZERO {
            let n = *rigid.normal;
            let along = belt - n * n.dot(belt);
            for contact in rigid.solver_contacts.iter_mut() {
                contact.tangent_velocity = along;
            }
        }
        *rigid.friction = mix(a.friction, a.friction_weight, b.friction, b.friction_weight).into();
        *rigid.restitution = Real::from(mix(
            a.elasticity,
            a.elasticity_weight,
            b.elasticity,
            b.elasticity_weight,
        ));
        let count = rigid.solver_contacts.len();
        if *rigid.restitution <= 0.0 || count < 2 {
            return;
        }
        let n = count as Real;
        let (mut at1, mut at2) = (Vector::ZERO, Vector::ZERO);
        let mut dist = Real::MAX;
        for contact in rigid.solver_contacts.iter() {
            at1 += contact.anchor1 / n;
            at2 += contact.anchor2 / n;
            dist = dist.min(contact.dist);
        }
        let approach = (velocity(body1, at1) - velocity(body2, at2)).dot(*rigid.normal);
        if approach > IMPACT_SPEED {
            rigid.solver_contacts.truncate(1);
            let contact = &mut rigid.solver_contacts[0];
            contact.anchor1 = at1;
            contact.anchor2 = at2;
            contact.dist = dist;
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
    constraints: Vec<constraints::Constraint>,
    /// Bodies a constraint acts on: always at 240 Hz.
    constrained: HashSet<RigidBodyHandle>,
}

impl World {
    pub fn new(shared: Rc<RefCell<Shared>>) -> Self {
        let params = IntegrationParameters {
            dt: 1.0 / PHYSICS_HZ,
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
            constraints: Vec::new(),
            constrained: HashSet::new(),
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
        let mut rigging = Vec::new();
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
            } else if is_constraint(instance.class()) {
                rigging.push(at);
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
        self.add_constraints(dom, db, &rigging);
        // A change to a constraint or its attachments rebuilds the world.
        for &referent in &rigging {
            self.joints.insert(referent);
            if let Some(instance) = dom.get(referent) {
                for name in ["Attachment0", "Attachment1"] {
                    if let Some(Variant::Ref(a)) = instance.properties().get(name) {
                        self.joints.insert(*a);
                    }
                }
            }
        }
    }

    fn add_assembly(&mut self, parts: &[PartData], member: &[usize], groups: &CollisionGroups) {
        let anchored = member.iter().any(|&i| parts[i].anchored);
        let referents: Vec<Ref> = member.iter().map(|&i| parts[i].referent).collect();
        let remote = !anchored && self.shared.borrow().ownership.remote(&referents);
        let root = root_part(parts, member);
        let root_pose = pose_of(&parts[root].cframe);
        let builder = if anchored {
            RigidBodyBuilder::fixed()
        } else if remote {
            RigidBodyBuilder::kinematic_position_based()
        } else {
            RigidBodyBuilder::dynamic()
                .angular_damping(ANGULAR_DAMPING)
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
                conveyor: if anchored {
                    vec3(parts[root].linear_velocity)
                } else {
                    Vector::ZERO
                },
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
                .density(density.into())
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
                .active_collision_types(collision_types(remote))
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
            parts: referents,
            anchored,
            remote,
            size: member
                .iter()
                .map(|&i| {
                    let s = parts[i].size;
                    Real::from(s.x.max(s.y).max(s.z))
                })
                .fold(0.0, Real::max),
            rate: std::cell::Cell::new(4),
            was_free: std::cell::Cell::new(false),
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
                self.bodies.get(assembly.body).map_or(own, |b| b.mass())
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
        let assembly = &self.assemblies[placed.assembly];
        if assembly.anchored && !angular {
            if let Some(body) = self.bodies.get(assembly.body) {
                for collider in body.colliders() {
                    let surface = self.colliders.get(*collider).map(|c| c.user_data as usize);
                    if let Some(surface) = surface.and_then(|s| self.surfaces.get_mut(s)) {
                        surface.conveyor = vec3(velocity);
                    }
                }
            }
            return;
        }
        if let Some(body) = self.bodies.get_mut(assembly.body) {
            if angular {
                body.set_angvel(vec3(velocity), true);
            } else {
                body.set_linvel(vec3(velocity), true);
            }
        }
    }

    /// Hands assemblies to and from other peers as ownership changed:
    /// a remote one is kinematic, moved where replication puts it.
    pub fn sync_ownership(&mut self) {
        let shared = self.shared.clone();
        let shared = shared.borrow();
        for assembly in self.assemblies.iter_mut().filter(|a| !a.anchored) {
            let remote = shared.ownership.remote(&assembly.parts);
            if remote == assembly.remote {
                continue;
            }
            assembly.remote = remote;
            let Some(body) = self.bodies.get_mut(assembly.body) else {
                continue;
            };
            body.set_body_type(
                if remote {
                    RigidBodyType::KinematicPositionBased
                } else {
                    RigidBodyType::Dynamic
                },
                true,
            );
            for collider in body.colliders().to_vec() {
                if let Some(c) = self.colliders.get_mut(collider) {
                    c.set_active_collision_types(collision_types(remote));
                }
            }
        }
    }

    /// Whether another peer simulates `part`.
    pub fn is_remote(&self, part: Ref) -> bool {
        self.parts
            .get(&part)
            .is_some_and(|p| self.assemblies[p.assembly].remote)
    }

    /// The parts of every unanchored assembly.
    pub fn unanchored(&self) -> Vec<Vec<Ref>> {
        self.assemblies
            .iter()
            .filter(|a| !a.anchored)
            .map(|a| a.parts.clone())
            .collect()
    }

    /// How far to move `body` to take it out of everything it has gone
    /// into: along the deepest contact's normal.
    fn way_out(&self, body: RigidBodyHandle) -> Vector {
        let Some(rigid) = self.bodies.get(body) else {
            return Vector::ZERO;
        };
        let velocity = rigid.linvel();
        let mut out = Vector::ZERO;
        for &handle in rigid.colliders() {
            let Some(collider) = self.colliders.get(handle) else {
                continue;
            };
            for pair in self.narrow.contact_pairs_with(handle) {
                let other = if pair.collider1 == handle { pair.collider2 } else { pair.collider1 };
                let Some(other) = self.colliders.get(other) else {
                    continue;
                };
                if other.parent() == Some(body) || other.is_sensor() {
                    continue;
                }
                let found = rapier3d::parry::query::contact(
                    collider.position(),
                    collider.shape(),
                    other.position(),
                    other.shape(),
                    0.0,
                );
                let Ok(Some(contact)) = found else {
                    continue;
                };
                let mut push = contact.normal1 * contact.dist;
                // Gone in past halfway, the shortest way out is the far
                // side: back along the way it came instead.
                if push.dot(velocity) > 0.0 && velocity.length() > 0.0 {
                    let mut from = *collider.position();
                    from.translation -= velocity * FRAME;
                    let options = rapier3d::parry::query::ShapeCastOptions {
                        max_time_of_impact: FRAME,
                        ..Default::default()
                    };
                    let hit = rapier3d::parry::query::cast_shapes(
                        &from,
                        velocity,
                        collider.shape(),
                        other.position(),
                        Vector::ZERO,
                        other.shape(),
                        options,
                    );
                    if let Ok(Some(hit)) = hit {
                        push = -velocity * (FRAME - hit.time_of_impact);
                    }
                }
                if push.length() > out.length() {
                    out = push;
                }
            }
        }
        out
    }

    /// Whether any of `assembly`'s parts is in contact with something.
    fn in_contact(&self, assembly: &Assembly) -> bool {
        let Some(body) = self.bodies.get(assembly.body) else {
            return false;
        };
        body.colliders().iter().any(|&collider| {
            self.narrow.contact_pairs_with(collider).any(|pair| {
                pair.has_any_active_contact()
                    || pair
                        .manifolds()
                        .iter()
                        .any(|m| m.points.iter().any(|p| p.dist < 0.0))
            })
        })
    }

    /// One frame of physics: `steps` solver steps of 1/240 s under
    /// `gravity` (studs/s², down), with the impulses scripts queued applied
    /// first. Returns the touches that began or ended.
    ///
    /// `adaptive` is Roblox's adaptive timestepping (its docs keep the
    /// criteria to themselves); ours is fitted to the FreeFall,
    /// FreeFallTrace and FreeFallRates goldens. Anything in contact or
    /// held by a constraint, and every assembly on the frame after a build,
    /// steps at 240 Hz. A free assembly starts at 60 Hz (one gravity kick a
    /// frame, then the substeps coast) and, while gravity speeds it up,
    /// moves to 120 Hz past `T120` and to 240 Hz past `T240` (speeds that
    /// grow with its size). Each switch up adds one kick of the new rate.
    pub fn step(
        &mut self,
        gravity: f64,
        steps: u32,
        adaptive: bool,
        drives: &mut [Drive],
    ) -> Vec<Touch> {
        self.shared.borrow_mut().generation += 1;
        // A world just built has no contacts yet: everything takes 240 Hz once.
        let adaptive = adaptive && !std::mem::replace(&mut self.fresh, false);
        let g = Vector::new(0.0, -gravity, 0.0);
        let h = self.params.dt;
        let mut kicked: Vec<(RigidBodyHandle, u32, bool)> = Vec::new();
        // Characters always step at 240 Hz (HumanoidDrop: a falling
        // character's first frames are 240 Hz free fall).
        let driven: HashSet<RigidBodyHandle> = drives
            .iter()
            .filter_map(|d| self.parts.get(&d.root))
            .map(|p| self.assemblies[p.assembly].body)
            .collect();
        for assembly in self.assemblies.iter().filter(|a| !a.anchored && !a.remote) {
            let free = adaptive
                && !driven.contains(&assembly.body)
                && !self.constrained.contains(&assembly.body)
                && !self.in_contact(assembly);
            let was_free = assembly.was_free.replace(free);
            let out = if was_free && !free {
                self.way_out(assembly.body)
            } else {
                Vector::ZERO
            };
            let Some(body) = self.bodies.get_mut(assembly.body) else {
                continue;
            };
            if out != Vector::ZERO {
                // Out of what it went into, and a fast one still takes its
                // free kick for this frame: it meets the floor at the speed
                // a free frame gives (TouchedOrder: 0.49 stud the second
                // frame up).
                let impact = -body.linvel().dot(out.normalize());
                body.set_translation(body.translation() + out, false);
                if impact > KICKED_IMPACT {
                    let kick = g * (h * assembly.rate.get() as Real * IMPACT_KICK);
                    body.set_linvel(body.linvel() + kick, false);
                }
            }
            if !free {
                assembly.rate.set(4);
                body.set_gravity_scale(1.0, false);
                continue;
            }
            body.set_gravity_scale(0.0, false);
            if body.is_sleeping() {
                continue;
            }
            let v = body.linvel();
            let speed = v.length();
            let mut rate = assembly.rate.get();
            let mut switched = false;
            if v.dot(g) > 0.0 {
                let next = match rate {
                    4 if speed > 29.86 * assembly.size + 15.14 => 2,
                    2 if speed > 91.0 * assembly.size + 49.0 => 1,
                    _ => rate,
                };
                if next != rate {
                    body.set_linvel(v + g * (h * next as Real), false);
                    rate = next;
                    switched = true;
                }
            }
            assembly.rate.set(rate);
            kicked.push((assembly.body, rate, switched));
        }
        let free_bodies: HashSet<RigidBodyHandle> = kicked.iter().map(|k| k.0).collect();
        let impulses = std::mem::take(&mut self.shared.borrow_mut().impulses);
        for (part, impulse, angular) in impulses {
            let Some(placed) = self.parts.get(&part) else {
                continue;
            };
            if let Some(body) = self.bodies.get_mut(self.assemblies[placed.assembly].body) {
                let impulse = Vector::from_array(impulse);
                if angular {
                    body.apply_torque_impulse(impulse, true);
                } else {
                    body.apply_impulse(impulse, true);
                }
            }
        }

        for substep in 0..steps {
            for &(handle, rate, _) in &kicked {
                if substep % rate == 0 {
                    if let Some(body) = self.bodies.get_mut(handle) {
                        body.set_linvel(body.linvel() + g * (h * rate as Real), false);
                    }
                }
            }
            self.drive_constraints(h, g, substep, &kicked);
            self.drive_characters(drives, h, gravity, substep);
            let mixer = Mixer(&self.surfaces, &free_bodies);
            self.pipeline.step(
                g,
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
            if assembly.anchored || assembly.remote {
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
            .filter(|a| !a.anchored && !a.remote)
            .flat_map(|a| a.parts.iter().copied())
    }

    /// The parts of every unanchored assembly whose position is below
    /// `height` (`Workspace.FallenPartsDestroyHeight`), the remote ones
    /// too when `remote` (the server destroys what clients drop).
    pub fn fallen(&self, height: f64, remote: bool) -> Vec<Ref> {
        let mut out = Vec::new();
        for assembly in self
            .assemblies
            .iter()
            .filter(|a| !a.anchored && (remote || !a.remote))
        {
            let Some(body) = self.bodies.get(assembly.body) else {
                continue;
            };
            for part in &assembly.parts {
                let Some(placed) = self.parts.get(part) else {
                    continue;
                };
                let y = (*body.position() * placed.offset).translation.y;
                if y < height {
                    out.push(*part);
                }
            }
        }
        out
    }
}

/// What a collider reports contacts with: a remote (kinematic) assembly
/// still touches anchored parts, so `Touched` fires where it lands.
fn collision_types(remote: bool) -> ActiveCollisionTypes {
    if remote {
        ActiveCollisionTypes::all()
    } else {
        ActiveCollisionTypes::default()
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
    Vector::new(v.x.into(), v.y.into(), v.z.into())
}

fn vector_data(v: Vector) -> Vector3Data {
    Vector3Data {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

/// `CFrameData`'s rotation is row-major.
pub(crate) fn pose_of(cframe: &CFrameData) -> Pose {
    let r = cframe.rotation.map(Real::from);
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
        rotation: [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z].map(|v| v as f32),
    }
}
