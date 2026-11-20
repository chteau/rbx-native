//! Humanoid characters in the world: the floor under a root part, what is
//! in front of it, and the drive that holds it at its hip height, walks and
//! turns it every substep. Every constant is fitted to the Humanoid goldens
//! (HumanoidStand, HumanoidSensor, HumanoidWalk, HumanoidWalkScale,
//! HumanoidTurn, HumanoidJump, HumanoidJumpScale, HumanoidClimb).

use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::*;
use rbx_dom::Ref;

use super::World;

/// The hip spring, studs/s² per stud below the standing height, and its
/// damping per stud/s. It only pushes up: a root above its standing height
/// falls under gravity (HumanoidSensor's `_d05`, `_d1`: plain free fall
/// until the root passes the standing height).
const HIP_SPRING: Real = 3500.0;
const HIP_DAMPING: Real = 110.0;
/// Each substep the walk closes this share of the gap to the target
/// velocity, by at most [`walk_cap`]. HumanoidWalk: 16 → 12.174 after one
/// frame, 15.924 after two; WalkSpeed 32: 12.174, 24.348, 31.757.
const WALK_GAIN: Real = 0.625;
/// AutoRotate turns at `TURN_GAIN` × the angle left (rad/s per radian),
/// the turn rate closing `TURN_LAG` of its gap each substep, so a turn
/// starts slow (HumanoidTurnRates `_w`: 12.2 rad/s at the end of the first
/// frame of a 90 degree turn, then 9.3 × the angle). Roblox's rigs take
/// one of two lags, 0.69 or 0.4 a substep, with nothing in the rig to say
/// which (identical rigs differ, the same rig changes mid-walk); 0.45 is
/// within two degrees of both, the 180 degree turn included.
const TURN_GAIN: Real = 8.4;
const TURN_LAG: Real = 0.45;
/// The floor sensor's footprint against the root's own half size: a root
/// walking off a ledge loses the floor a frame before it starts to drop
/// (HumanoidSensor `ledge_states`, `_ledge`).
const FOOTPRINT: Real = 0.6;
/// A root this close to its standing height and this slow is settled: the
/// spring alone holds it, gravity off, so it comes to rest instead of
/// chattering between spring and gravity (HumanoidStand `_vy`: 0.001).
const HOLD_BAND: Real = 0.05;
const SETTLED_SPEED: Real = 1.0;
/// Half the height of the box the floor sensor casts.
const SENSOR_THICKNESS: Real = 0.01;

/// What the drive does with a character this substep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DriveMode {
    /// A plain rigid body: physics, ragdoll, platform standing, dead.
    #[default]
    Off,
    /// On the ground: hip spring, walking on the floor's plane.
    Ground,
    /// In the air: gravity, walking still steers.
    Air,
    /// On a truss: no gravity, up or down at `climb`.
    Climb,
    /// In terrain water: the water holds it up, `walk` steers it.
    Swim,
}

/// One character's drive, rebuilt every frame by the humanoid code.
#[derive(Clone, Debug)]
pub struct Drive {
    pub root: Ref,
    pub mode: DriveMode,
    /// Root centre to the floor when standing.
    pub stand: Real,
    /// The root's half size along its own X and Z.
    pub half: [Real; 2],
    /// Target velocity on flat ground: MoveDirection × WalkSpeed.
    pub walk: [Real; 3],
    /// `MaxSlopeAngle`, radians.
    pub max_slope: Real,
    /// Facing, as a turn about +Y (0 faces -Z).
    pub yaw: Real,
    /// The facing to turn toward.
    pub turn: Option<Real>,
    /// How fast the facing turns, rad/s.
    pub spin: Real,
    /// A jump's take-off speed and how far the root is lifted, used on the
    /// first substep.
    pub launch: Option<[Real; 2]>,
    /// On a truss: how far to walk in to it this frame, along the facing.
    pub approach: Real,
    /// Vertical speed on a truss.
    pub climb: Real,
    /// In water: how fast vertical motion dies out, per second.
    pub swim_damping: Real,
}

/// What the sensor found under a root.
#[derive(Clone, Copy, Debug)]
pub struct Floor {
    /// Root centre down to the floor's surface.
    pub distance: Real,
    pub normal: [Real; 3],
    pub part: Ref,
    pub friction: Real,
    /// How fast the floor under the root moves (a platform, or a conveyor's
    /// belt): walking is relative to it (HumanoidPlatform, Conveyor).
    pub velocity: [Real; 3],
    /// The `Enum.Material` value of the terrain under it, when it is terrain.
    pub terrain_material: Option<u32>,
}

/// Velocity gained per substep at most, from the floor's friction and the
/// character's density: 3.0435 stud/s on plastic (0.3), 0.9125 on ice
/// (0.02) (HumanoidWalk, HumanoidWalkScale `ice`), a line through both, for
/// plastic's 0.7; a denser rig grips harder in proportion (HumanoidTurnRates
/// `d2` reaches 15.61 stud/s in its first frame, 8.7 a substep, and
/// HumanoidWalkScale `heavy` isn't held back at all).
fn walk_cap(friction: Real, density: Real) -> Real {
    (0.7603 + 7.6107 * friction) * density / 0.7
}

fn wrap(angle: Real) -> Real {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

impl World {
    fn body_of(&self, part: Ref) -> Option<(RigidBodyHandle, Pose)> {
        let placed = self.parts.get(&part)?;
        let assembly = &self.assemblies[placed.assembly];
        (!assembly.anchored).then_some((assembly.body, placed.offset))
    }

    /// Where `part` is in the world.
    pub fn part_pose(&self, part: Ref) -> Option<Pose> {
        let placed = self.parts.get(&part)?;
        let body = self.bodies.get(self.assemblies[placed.assembly].body)?;
        Some(*body.position() * placed.offset)
    }

    /// The root's assembly velocity.
    pub fn part_velocity(&self, part: Ref) -> Option<[Real; 3]> {
        let placed = self.parts.get(&part)?;
        let body = self.bodies.get(self.assemblies[placed.assembly].body)?;
        Some(body.linvel().to_array())
    }

    /// The floor under `root` within `reach` of its standing height: a thin
    /// box the size of the root's footprint cast straight down.
    pub fn floor_under(
        &self,
        root: Ref,
        stand: Real,
        half: [Real; 2],
        yaw: Real,
        reach: Real,
    ) -> Option<Floor> {
        let placed = self.parts.get(&root)?;
        let body = self.assemblies[placed.assembly].body;
        let at = self.part_pose(root)?;
        let shape = Cuboid::new(Vector::new(
            half[0] * FOOTPRINT,
            SENSOR_THICKNESS,
            half[1] * FOOTPRINT,
        ));
        let pose = Pose::from_parts(at.translation, Rotation::from_rotation_y(yaw));
        let filter = QueryFilter::default()
            .exclude_rigid_body(body)
            .exclude_sensors();
        let pipeline = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            filter,
        );
        let options = ShapeCastOptions {
            max_time_of_impact: stand + reach - SENSOR_THICKNESS,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: true,
        };
        let (collider, hit) =
            pipeline.cast_shape(&pose, Vector::new(0.0, -1.0, 0.0), &shape, options)?;
        // From the root's centre, not the thin box's bottom.
        let distance = hit.time_of_impact + SENSOR_THICKNESS;
        let part = *self.by_collider.get(&collider)?;
        let terrain_material = self.terrain_material(collider).map(|m| m.enum_value());
        let mut normal = hit.normal2;
        if normal.y < 0.0 {
            normal = -normal;
        }
        let collider = self.colliders.get(collider)?;
        let surface = self.surfaces.get(collider.user_data as usize);
        let friction = surface.map_or(0.3, |s| Real::from(s.friction));
        let below = at.translation - Vector::new(0.0, distance, 0.0);
        let velocity = match collider.parent().and_then(|b| self.bodies.get(b)) {
            Some(body) if body.is_dynamic() || body.is_kinematic() => body.velocity_at_point(below),
            _ => surface.map_or(Vector::ZERO, |s| s.conveyor),
        };
        Some(Floor {
            distance,
            normal: normal.to_array(),
            part,
            friction,
            velocity: velocity.to_array(),
            terrain_material,
        })
    }

    /// The nearest part straight ahead of `root` (along its facing) within
    /// `reach` of its front face, and how far that is from the face.
    pub fn part_ahead(
        &self,
        root: Ref,
        half_depth: Real,
        yaw: Real,
        reach: Real,
    ) -> Option<(Ref, Real)> {
        let placed = self.parts.get(&root)?;
        let body = self.assemblies[placed.assembly].body;
        let at = self.part_pose(root)?;
        let look = Vector::new(-yaw.sin(), 0.0, -yaw.cos());
        let filter = QueryFilter::default()
            .exclude_rigid_body(body)
            .exclude_sensors();
        let pipeline = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            filter,
        );
        let ray = Ray::new(at.translation, look);
        let (collider, toi) = pipeline.cast_ray(&ray, half_depth + reach, true)?;
        Some((
            *self.by_collider.get(&collider)?,
            (toi - half_depth).max(0.0),
        ))
    }

    /// One substep of every character's drive, before the solver runs.
    pub(super) fn drive_characters(
        &mut self,
        drives: &mut [Drive],
        h: Real,
        gravity: Real,
        substep: u32,
    ) {
        for drive in drives.iter_mut() {
            if drive.mode == DriveMode::Off {
                continue;
            }
            let Some((handle, offset)) = self.body_of(drive.root) else {
                continue;
            };
            match drive.turn {
                Some(target) => {
                    let mut left = wrap(target - drive.yaw);
                    // Straight behind, it turns to its left (HumanoidTurnRates `back`).
                    if left <= 1e-9 - std::f64::consts::PI {
                        left = std::f64::consts::PI;
                    }
                    drive.spin += TURN_LAG * (TURN_GAIN * left - drive.spin);
                    drive.yaw += drive.spin * h;
                }
                None => drive.spin = 0.0,
            }
            let floor = match drive.mode {
                DriveMode::Ground => {
                    self.floor_under(drive.root, drive.stand, drive.half, drive.yaw, HOLD_BAND)
                }
                _ => None,
            };
            let volume: Real = self.bodies.get(handle).map_or(0.0, |b| {
                b.colliders()
                    .iter()
                    .filter_map(|c| self.colliders.get(*c))
                    .map(|c| c.volume())
                    .sum()
            });
            let Some(body) = self.bodies.get_mut(handle) else {
                continue;
            };
            let density = if volume > 0.0 {
                body.mass() / volume
            } else {
                0.7
            };
            let root_now = *body.position() * offset;
            let upright =
                Pose::from_parts(root_now.translation, Rotation::from_rotation_y(drive.yaw));
            if root_now.rotation.angle_between(upright.rotation) > 1e-9 {
                body.set_position(upright * offset.inverse(), false);
            }
            body.set_angvel(Vector::ZERO, false);
            // A character never sleeps: its drive moves it every substep.
            body.activation_mut().normalized_linear_threshold = -1.0;
            body.activation_mut().angular_threshold = -1.0;
            let mut v = body.linvel();
            // Gravity is off while the root is settled on its floor; on a
            // truss and a jump's first substep the drive cancels it.
            let mut weightless = false;
            let carried = floor.map_or(Vector::ZERO, |f| {
                let v = Vector::from_array(f.velocity);
                Vector::new(v.x, 0.0, v.z)
            });
            let walk = Vector::from_array(drive.walk);
            let mut target = walk + carried;
            let mut cap = walk_cap(0.3, density);
            match drive.mode {
                DriveMode::Ground => {
                    if let Some(floor) = floor {
                        cap = walk_cap(floor.friction, density);
                        let n = Vector::from_array(floor.normal);
                        let walkable = n.y >= drive.max_slope.cos();
                        if walkable && walk.length() > 0.0 {
                            let along = walk - n * n.dot(walk);
                            if along.length() > 1e-9 {
                                target = along * (walk.length() / along.length()) + carried;
                            }
                        }
                        let below = drive.stand - floor.distance;
                        let a = HIP_SPRING * below + HIP_DAMPING * (target.y - v.y);
                        let settled = below.abs() < HOLD_BAND
                            && v.y.abs() < SETTLED_SPEED
                            && target.y.abs() < 1e-9;
                        if walkable && settled {
                            v.y += a * h;
                            weightless = true;
                        } else if walkable && (below > 0.0 || (below > -HOLD_BAND && v.y <= 0.0)) {
                            v.y += (a + gravity) * h;
                        }
                    }
                }
                DriveMode::Climb => {
                    let pace = drive.approach / (4.0 * h);
                    v = Vector::new(
                        -drive.yaw.sin() * pace,
                        drive.climb + gravity * h,
                        -drive.yaw.cos() * pace,
                    );
                    target = Vector::new(0.0, v.y, 0.0);
                }
                DriveMode::Swim => v.y *= (-drive.swim_damping * h).exp(),
                DriveMode::Air | DriveMode::Off => {}
            }
            if substep == 0 {
                if let Some([speed, lift]) = drive.launch.take() {
                    v.y = speed + gravity * h;
                    if lift != 0.0 {
                        let up = body.translation() + Vector::new(0.0, lift, 0.0);
                        body.set_translation(up, false);
                    }
                }
            }
            if drive.mode != DriveMode::Climb {
                let gap = Vector::new(target.x - v.x, 0.0, target.z - v.z);
                let size = gap.length();
                if size > 1e-12 {
                    let step = (WALK_GAIN * size).min(cap);
                    v += gap * (step / size);
                }
            }
            body.set_gravity_scale(if weightless { 0.0 } else { 1.0 }, false);
            body.set_linvel(v, true);
        }
    }
}
