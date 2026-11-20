//! Roblox's constraints and movers on Rapier.
//!
//! - `HingeConstraint`, `PrismaticConstraint`, `BallSocketConstraint` and
//!   `RopeConstraint` are Rapier impulse joints between the attachments'
//!   frames (the attachment's X axis is the hinge or slide axis, as the
//!   docs say). Hinge and prismatic motors and servos drive the joint's
//!   velocity motor every substep; a servo moves at `AngularSpeed`/`Speed`
//!   towards its target, the HingeServo golden.
//! - `SpringConstraint` is Hooke's law plus damping along the line between
//!   the attachments, applied as an impulse every substep (the Spring
//!   golden).
//! - `AlignPosition`/`AlignOrientation` pull like a critically damped
//!   spring whose natural frequency is half the `Responsiveness`, fitted to
//!   the AlignPosition and AlignOrientation goldens (both follow the same
//!   curve). `RigidityEnabled` stands in as a responsiveness of 200.
//! - `LinearVelocity`/`AngularVelocity` reach their target within a substep
//!   as far as `MaxForce`/`MaxTorque` allow; `VectorForce`/`Torque` push
//!   every substep.
//!
//! Parameters are read again from the DOM every frame
//! ([`World::refresh_constraints`]), so a script changing `TargetAngle` or
//! `VectorVelocity` takes effect on the next step without a rebuild.

use rapier3d::prelude::*;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{pose_of, vec3, World};

/// Natural frequency of hinge and ball-socket joints, Hz.
const JOINT_HZ: f64 = 60.0;

/// Responsiveness `RigidityEnabled` stands for.
const RIGID_RESPONSIVENESS: Real = 200.0;
/// Velocity motor gain: high enough to hold a target within a substep.
const MOTOR_GAIN: Real = 1.0e4;

/// One end of a constraint: the body its attachment's part belongs to and
/// the attachment's frame in that body's frame.
#[derive(Clone, Copy)]
pub(super) struct End {
    body: RigidBodyHandle,
    frame: Pose,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Actuator {
    Free,
    Motor {
        velocity: Real,
        max: Real,
    },
    Servo {
        target: Real,
        speed: Real,
        max: Real,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Relative {
    World,
    Attachment0,
    Attachment1,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drive {
    Hinge(Actuator),
    Prismatic(Actuator),
    BallSocket,
    Rope {
        length: Real,
    },
    Spring {
        free_length: Real,
        stiffness: Real,
        damping: Real,
        max_force: Real,
    },
    AlignPosition {
        target: Option<Vector>,
        omega: Real,
        max_force: Real,
        max_velocity: Real,
        reaction: bool,
    },
    AlignOrientation {
        target: Option<Rotation>,
        omega: Real,
        max_torque: Real,
        reaction: bool,
    },
    LinearVelocity {
        velocity: Vector,
        max_force: Real,
        relative: Relative,
    },
    AngularVelocity {
        velocity: Vector,
        max_torque: Real,
        relative: Relative,
    },
    VectorForce {
        force: Vector,
        relative: Relative,
        at_center: bool,
    },
    Torque {
        torque: Vector,
        relative: Relative,
    },
}

#[derive(Clone, Copy)]
pub(super) struct Constraint {
    referent: Ref,
    ends: [End; 2],
    /// Whether the second attachment exists (a one-attachment mover has
    /// only `ends[0]` meaningful).
    two: bool,
    joint: Option<ImpulseJointHandle>,
    drive: Drive,
}

fn prop<'a>(
    db: &'a ReflectionDatabase,
    instance: &'a Instance,
    name: &'a str,
) -> Option<&'a Variant> {
    db.stored_or_default(instance, name).map(|(_, v)| v)
}

fn num(db: &ReflectionDatabase, instance: &Instance, name: &str, default: Real) -> Real {
    match prop(db, instance, name) {
        Some(Variant::Float32(v)) => Real::from(*v),
        Some(Variant::Float64(v)) => *v,
        Some(Variant::Int32(v)) => *v as Real,
        _ => default,
    }
}

fn flag(db: &ReflectionDatabase, instance: &Instance, name: &str, default: bool) -> bool {
    match prop(db, instance, name) {
        Some(Variant::Bool(v)) => *v,
        _ => default,
    }
}

fn vector(db: &ReflectionDatabase, instance: &Instance, name: &str) -> Vector {
    match prop(db, instance, name) {
        Some(Variant::Vector3(v)) => vec3(*v),
        _ => Vector::ZERO,
    }
}

fn enum_name(db: &ReflectionDatabase, instance: &Instance, name: &str, enum_: &str) -> String {
    match prop(db, instance, name) {
        Some(Variant::Enum(raw)) => db.enum_name(enum_, *raw).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// `inf` (`math.huge`) as Rapier's largest force.
fn cap(value: Real) -> Real {
    if value.is_finite() {
        value.max(0.0)
    } else {
        Real::MAX
    }
}

fn relative(db: &ReflectionDatabase, instance: &Instance, default: Relative) -> Relative {
    match enum_name(db, instance, "RelativeTo", "ActuatorRelativeTo").as_str() {
        "World" => Relative::World,
        "Attachment0" => Relative::Attachment0,
        "Attachment1" => Relative::Attachment1,
        _ => default,
    }
}

fn actuator(db: &ReflectionDatabase, instance: &Instance, angular: bool) -> Actuator {
    let (velocity, motor_max, target, speed, servo_max) = if angular {
        (
            "AngularVelocity",
            "MotorMaxTorque",
            "TargetAngle",
            "AngularSpeed",
            "ServoMaxTorque",
        )
    } else {
        (
            "Velocity",
            "MotorMaxForce",
            "TargetPosition",
            "Speed",
            "ServoMaxForce",
        )
    };
    let unit = if angular {
        std::f64::consts::PI / 180.0
    } else {
        1.0
    };
    match enum_name(db, instance, "ActuatorType", "ActuatorType").as_str() {
        "Motor" => Actuator::Motor {
            velocity: num(db, instance, velocity, 0.0),
            max: cap(num(db, instance, motor_max, 0.0)),
        },
        "Servo" => Actuator::Servo {
            target: num(db, instance, target, 0.0) * unit,
            speed: num(db, instance, speed, 0.0),
            max: cap(num(db, instance, servo_max, 0.0)),
        },
        _ => Actuator::Free,
    }
}

/// What `instance` asks for this frame, or `None` when it isn't a
/// constraint we drive (or is disabled).
fn read_drive(db: &ReflectionDatabase, instance: &Instance) -> Option<Drive> {
    if !flag(db, instance, "Enabled", true) {
        return None;
    }
    let omega = |rigid: bool| {
        if rigid {
            RIGID_RESPONSIVENESS / 2.0
        } else {
            num(db, instance, "Responsiveness", 10.0) / 2.0
        }
    };
    let one = |mode_enum: &str| enum_name(db, instance, "Mode", mode_enum) == "OneAttachment";
    Some(match instance.class() {
        "HingeConstraint" => Drive::Hinge(actuator(db, instance, true)),
        "PrismaticConstraint" => Drive::Prismatic(actuator(db, instance, false)),
        "BallSocketConstraint" => Drive::BallSocket,
        "RopeConstraint" => Drive::Rope {
            length: num(db, instance, "Length", 5.0),
        },
        "SpringConstraint" => Drive::Spring {
            free_length: num(db, instance, "FreeLength", 1.0),
            stiffness: num(db, instance, "Stiffness", 0.0),
            damping: num(db, instance, "Damping", 0.0),
            max_force: cap(num(db, instance, "MaxForce", Real::INFINITY)),
        },
        "AlignPosition" => Drive::AlignPosition {
            target: one("PositionAlignmentMode").then(|| vector(db, instance, "Position")),
            omega: omega(flag(db, instance, "RigidityEnabled", false)),
            max_force: cap(num(db, instance, "MaxForce", 10000.0)),
            max_velocity: cap(num(db, instance, "MaxVelocity", Real::INFINITY)),
            reaction: flag(db, instance, "ReactionForceEnabled", false),
        },
        "AlignOrientation" => Drive::AlignOrientation {
            target: one("OrientationAlignmentMode").then(|| match prop(db, instance, "CFrame") {
                Some(Variant::CFrame(cf)) => pose_of(cf).rotation,
                _ => Rotation::IDENTITY,
            }),
            omega: omega(flag(db, instance, "RigidityEnabled", false)),
            max_torque: cap(num(db, instance, "MaxTorque", 10000.0)),
            reaction: flag(db, instance, "ReactionTorqueEnabled", false),
        },
        "LinearVelocity" => Drive::LinearVelocity {
            velocity: vector(db, instance, "VectorVelocity"),
            max_force: cap(num(db, instance, "MaxForce", 1000.0)),
            relative: relative(db, instance, Relative::World),
        },
        "AngularVelocity" => Drive::AngularVelocity {
            velocity: vector(db, instance, "AngularVelocity"),
            max_torque: cap(num(db, instance, "MaxTorque", 0.0)),
            relative: relative(db, instance, Relative::World),
        },
        "VectorForce" => Drive::VectorForce {
            force: vector(db, instance, "Force"),
            relative: relative(db, instance, Relative::Attachment0),
            at_center: flag(db, instance, "ApplyAtCenterOfMass", false),
        },
        "Torque" => Drive::Torque {
            torque: vector(db, instance, "Torque"),
            relative: relative(db, instance, Relative::Attachment0),
        },
        _ => return None,
    })
}

/// Whether `class` is one of the constraints this module drives.
pub(crate) fn is_constraint(class: &str) -> bool {
    matches!(
        class,
        "HingeConstraint"
            | "PrismaticConstraint"
            | "BallSocketConstraint"
            | "RopeConstraint"
            | "SpringConstraint"
            | "AlignPosition"
            | "AlignOrientation"
            | "LinearVelocity"
            | "AngularVelocity"
            | "VectorForce"
            | "Torque"
    )
}

fn attachment(instance: &Instance, name: &str) -> Option<Ref> {
    match instance.properties().get(name) {
        Some(Variant::Ref(r)) => Some(*r),
        _ => None,
    }
}

/// Signed angle of `b` about `axis` from `a` (both perpendicular-ish to it).
fn angle_about(axis: Vector, a: Vector, b: Vector) -> Real {
    a.cross(b).dot(axis).atan2(a.dot(b))
}

impl World {
    /// The end an attachment gives: its part's body and its frame there.
    fn end_of(&self, dom: &WeakDom, db: &ReflectionDatabase, attachment: Ref) -> Option<End> {
        let instance = dom.get(attachment)?;
        if !db.is_subclass_of(instance.class(), "Attachment") {
            return None;
        }
        let placed = self.parts.get(&dom.parent(attachment)?)?;
        let local = match prop(db, instance, "CFrame") {
            Some(Variant::CFrame(cf)) => pose_of(cf),
            _ => Pose::IDENTITY,
        };
        Some(End {
            body: self.assemblies[placed.assembly].body,
            frame: placed.offset * local,
        })
    }

    /// Builds every enabled constraint under the workspace whose
    /// attachments sit on simulated parts. Called at the end of a rebuild.
    pub(super) fn add_constraints(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        found: &[Ref],
    ) {
        for &referent in found {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            let Some(drive) = read_drive(db, instance) else {
                continue;
            };
            let Some(end0) =
                attachment(instance, "Attachment0").and_then(|a| self.end_of(dom, db, a))
            else {
                continue;
            };
            let end1 = attachment(instance, "Attachment1").and_then(|a| self.end_of(dom, db, a));
            let needs_two = matches!(
                drive,
                Drive::Hinge(_)
                    | Drive::Prismatic(_)
                    | Drive::BallSocket
                    | Drive::Rope { .. }
                    | Drive::Spring { .. }
            ) || matches!(drive, Drive::AlignPosition { target: None, .. })
                || matches!(drive, Drive::AlignOrientation { target: None, .. });
            if needs_two && end1.is_none() {
                continue;
            }
            let ends = [end0, end1.unwrap_or(end0)];
            let joint = self.make_joint(&drive, &ends, instance, db);
            for end in ends {
                if let Some(body) = self.bodies.get_mut(end.body) {
                    body.wake_up(true);
                }
            }
            // Force and spin movers and a slack rope leave a body on adaptive
            // stepping (the AngularVelocity, VectorForce and Rope goldens fall
            // as a free block does).
            if !matches!(
                drive,
                Drive::Rope { .. }
                    | Drive::AngularVelocity { .. }
                    | Drive::VectorForce { .. }
                    | Drive::Torque { .. }
            ) {
                self.constrained.extend(ends.iter().map(|e| e.body));
            }
            self.constraints.push(Constraint {
                referent,
                ends,
                two: end1.is_some(),
                joint,
                drive,
            });
        }
    }

    fn make_joint(
        &mut self,
        drive: &Drive,
        ends: &[End; 2],
        instance: &Instance,
        db: &ReflectionDatabase,
    ) -> Option<ImpulseJointHandle> {
        if ends[0].body == ends[1].body {
            return None;
        }
        let limits = flag(db, instance, "LimitsEnabled", false);
        let joint: GenericJoint = match drive {
            Drive::Hinge(_) => {
                let mut b = GenericJointBuilder::new(JointAxesMask::LOCKED_REVOLUTE_AXES)
                    .local_frame1(ends[0].frame)
                    .local_frame2(ends[1].frame);
                if limits {
                    let deg = std::f64::consts::PI / 180.0;
                    b = b.limits(
                        JointAxis::AngX,
                        [
                            num(db, instance, "LowerAngle", -45.0) * deg,
                            num(db, instance, "UpperAngle", 45.0) * deg,
                        ],
                    );
                }
                b.build()
            }
            Drive::Prismatic(_) => {
                let mut b = GenericJointBuilder::new(JointAxesMask::LOCKED_PRISMATIC_AXES)
                    .local_frame1(ends[0].frame)
                    .local_frame2(ends[1].frame);
                if limits {
                    b = b.limits(
                        JointAxis::LinX,
                        [
                            num(db, instance, "LowerLimit", 0.0),
                            num(db, instance, "UpperLimit", 5.0),
                        ],
                    );
                }
                b.build()
            }
            Drive::BallSocket => GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                .local_frame1(ends[0].frame)
                .local_frame2(ends[1].frame)
                .build(),
            Drive::Rope { length } => RopeJointBuilder::new(*length)
                .local_anchor1(ends[0].frame.translation)
                .local_anchor2(ends[1].frame.translation)
                .build()
                .into(),
            _ => return None,
        };
        // Roblox's sockets and hinges give a little (a 4 stud BallSocket arm
        // stretches to 4.046 at the bottom of its swing); a 60 Hz spring
        // keeps the HingeFree swing within 0.08 stud over 30 frames.
        let mut joint = joint;
        if matches!(drive, Drive::Hinge(_) | Drive::BallSocket) {
            let _ = joint.set_softness(SpringCoefficients::new(JOINT_HZ, 1.0));
        }
        Some(
            self.impulse_joints
                .insert(ends[0].body, ends[1].body, joint, true),
        )
    }

    /// Re-reads every constraint's parameters (scripts change them between
    /// frames), keeping the joints.
    pub(crate) fn refresh_constraints(&mut self, dom: &WeakDom, db: &ReflectionDatabase) {
        for constraint in &mut self.constraints {
            let Some(drive) = dom
                .get(constraint.referent)
                .and_then(|instance| read_drive(db, instance))
            else {
                continue;
            };
            if std::mem::discriminant(&drive) == std::mem::discriminant(&constraint.drive) {
                constraint.drive = drive;
            }
            if let (Drive::Rope { length }, Some(handle)) = (drive, constraint.joint) {
                if let Some(joint) = self.impulse_joints.get_mut(handle, true) {
                    joint.data.set_limits(JointAxis::LinX, [0.0, length]);
                }
            }
        }
    }

    /// World frame of `end` now.
    fn frame_of(&self, end: &End) -> Pose {
        self.bodies
            .get(end.body)
            .map(|b| *b.position() * end.frame)
            .unwrap_or(end.frame)
    }

    /// The hinge angle (radians) or slide position of a joint constraint.
    fn joint_position(&self, constraint: &Constraint) -> Real {
        let (f0, f1) = (
            self.frame_of(&constraint.ends[0]),
            self.frame_of(&constraint.ends[1]),
        );
        let axis = f0.rotation * Vector::X;
        match constraint.drive {
            Drive::Prismatic(_) => (f1.translation - f0.translation).dot(axis),
            _ => angle_about(axis, f0.rotation * Vector::Y, f1.rotation * Vector::Y),
        }
    }

    /// One substep of `h` seconds of every constraint's drive, before the
    /// solver step adds `g` (gravity) to the bodies it acts on.
    /// `kicked` are the bodies on adaptive stepping this frame (rate, and
    /// whether it switched up): a VectorForce on one pushes with the
    /// gravity kicks, switch kick included (VectorForce golden).
    pub(super) fn drive_constraints(
        &mut self,
        h: Real,
        g: Vector,
        substep: u32,
        kicked: &[(RigidBodyHandle, u32, bool)],
    ) {
        for constraint in &self.constraints {
            for end in constraint.ends {
                if let Some(body) = self.bodies.get_mut(end.body) {
                    body.reset_forces(false);
                    body.reset_torques(false);
                }
            }
        }
        for i in 0..self.constraints.len() {
            let constraint = &self.constraints[i].clone();
            let drive = constraint.drive;
            match drive {
                Drive::Hinge(actuator) | Drive::Prismatic(actuator) => {
                    let axis = if matches!(drive, Drive::Hinge(_)) {
                        JointAxis::AngX
                    } else {
                        JointAxis::LinX
                    };
                    let (velocity, max) = match actuator {
                        Actuator::Free => (0.0, 0.0),
                        Actuator::Motor { velocity, max } => (velocity, max),
                        Actuator::Servo { target, speed, max } => {
                            let mut error = target - self.joint_position(constraint);
                            if axis == JointAxis::AngX {
                                error = (error + std::f64::consts::PI)
                                    .rem_euclid(std::f64::consts::TAU)
                                    - std::f64::consts::PI;
                            }
                            ((error / h).clamp(-speed, speed), max)
                        }
                    };
                    if let Some(handle) = constraint.joint {
                        if let Some(joint) = self.impulse_joints.get_mut(handle, true) {
                            joint.data.set_motor_velocity(axis, velocity, MOTOR_GAIN);
                            joint.data.set_motor_max_force(axis, max);
                        }
                    }
                }
                Drive::Spring {
                    free_length,
                    stiffness,
                    damping,
                    max_force,
                } => {
                    let (p0, p1) = (
                        self.frame_of(&constraint.ends[0]).translation,
                        self.frame_of(&constraint.ends[1]).translation,
                    );
                    let d = p1 - p0;
                    let length = d.length();
                    if length < 1e-6 {
                        continue;
                    }
                    let n = d / length;
                    let v = |end: &End, p: Vector| {
                        self.bodies
                            .get(end.body)
                            .map_or(Vector::ZERO, |b| b.velocity_at_point(p))
                    };
                    let closing = (v(&constraint.ends[1], p1) - v(&constraint.ends[0], p0)).dot(n);
                    let force = (stiffness * (length - free_length) + damping * closing)
                        .clamp(-max_force, max_force);
                    let impulse = n * (force * h);
                    let ends = constraint.ends;
                    self.push(ends[0].body, impulse, Some(p0), h);
                    self.push(ends[1].body, -impulse, Some(p1), h);
                }
                Drive::AlignPosition {
                    target,
                    omega,
                    max_force,
                    max_velocity,
                    reaction,
                } => {
                    let ends = constraint.ends;
                    let p0 = self.frame_of(&ends[0]).translation;
                    let goal = target.unwrap_or_else(|| self.frame_of(&ends[1]).translation);
                    let other = if target.is_none() {
                        self.bodies
                            .get(ends[1].body)
                            .map_or(Vector::ZERO, |b| b.velocity_at_point(goal))
                    } else {
                        Vector::ZERO
                    };
                    let Some(body) = self.bodies.get(ends[0].body) else {
                        continue;
                    };
                    let v = body.linvel() - other;
                    let mass = body.mass();
                    // The solver adds gravity after this: hold against it.
                    let mut dv = (omega * omega * (goal - p0) - 2.0 * omega * v) * h
                        - g * (h * body.gravity_scale());
                    if (mass * dv.length() / h) > max_force {
                        dv *= max_force * h / (mass * dv.length());
                    }
                    let after = v + dv;
                    if after.length() > max_velocity {
                        dv = after.normalize() * max_velocity - v;
                    }
                    self.push(ends[0].body, dv * mass, None, h);
                    if reaction && constraint.two {
                        self.push(ends[1].body, -dv * mass, None, h);
                    }
                }
                Drive::AlignOrientation {
                    target,
                    omega,
                    max_torque,
                    reaction,
                } => {
                    let ends = constraint.ends;
                    let current = self.frame_of(&ends[0]).rotation;
                    let goal = target.unwrap_or_else(|| self.frame_of(&ends[1]).rotation);
                    let mut error = goal * current.inverse();
                    if error.w < 0.0 {
                        error = -error;
                    }
                    let (axis, angle) = error.to_axis_angle();
                    let Some(body) = self.bodies.get(ends[0].body) else {
                        continue;
                    };
                    let w = body.angvel();
                    let alpha = omega * omega * angle * axis - 2.0 * omega * w;
                    let dw = self.capped_spin(ends[0].body, alpha * h, max_torque, h);
                    self.spin(ends[0].body, dw, h);
                    if reaction && constraint.two {
                        self.spin(ends[1].body, -dw, h);
                    }
                }
                Drive::LinearVelocity {
                    velocity,
                    max_force,
                    relative,
                } => {
                    let end = constraint.ends[0];
                    let goal = self.oriented(constraint, velocity, relative);
                    let Some(body) = self.bodies.get(end.body) else {
                        continue;
                    };
                    let mass = body.mass();
                    let mut dv = goal - body.linvel() - g * (h * body.gravity_scale());
                    if mass * dv.length() / h > max_force {
                        dv *= max_force * h / (mass * dv.length());
                    }
                    self.push(end.body, dv * mass, None, h);
                }
                Drive::AngularVelocity {
                    velocity,
                    max_torque,
                    relative,
                } => {
                    let end = constraint.ends[0];
                    let goal = self.oriented(constraint, velocity, relative);
                    let Some(body) = self.bodies.get(end.body) else {
                        continue;
                    };
                    let dw = self.capped_spin(end.body, goal - body.angvel(), max_torque, h);
                    self.spin(end.body, dw, h);
                }
                Drive::VectorForce {
                    force,
                    relative,
                    at_center,
                } => {
                    let end = constraint.ends[0];
                    let kicks = match kicked.iter().find(|k| k.0 == end.body) {
                        Some(&(_, rate, switched)) if substep % rate == 0 => {
                            rate * if switched && substep == 0 { 2 } else { 1 }
                        }
                        Some(_) => continue,
                        None => 1,
                    };
                    let force = self.oriented(constraint, force, relative);
                    let at = (!at_center).then(|| self.frame_of(&end).translation);
                    self.push(end.body, force * (h * kicks as Real), at, h);
                }
                Drive::Torque { torque, relative } => {
                    let end = constraint.ends[0];
                    let torque = self.oriented(constraint, torque, relative);
                    if let Some(body) = self.bodies.get_mut(end.body) {
                        if body.is_dynamic() {
                            body.add_torque(torque, true);
                        }
                    }
                }
                Drive::BallSocket | Drive::Rope { .. } => {}
            }
        }
    }

    /// `v` in world space, from the frame `relative` names.
    fn oriented(&self, constraint: &Constraint, v: Vector, relative: Relative) -> Vector {
        match relative {
            Relative::World => v,
            Relative::Attachment0 => self.frame_of(&constraint.ends[0]).rotation * v,
            Relative::Attachment1 => self.frame_of(&constraint.ends[1]).rotation * v,
        }
    }

    /// `dw` scaled down so the torque it takes over `h` stays within `max`.
    fn capped_spin(&self, body: RigidBodyHandle, dw: Vector, max: Real, h: Real) -> Vector {
        if max == Real::MAX {
            return dw;
        }
        let Some(body) = self.bodies.get(body) else {
            return dw;
        };
        let inertia = body.mass_properties().effective_angular_inertia();
        let torque = (inertia * dw).length() / h;
        if torque > max && torque > 0.0 {
            dw * (max / torque)
        } else {
            dw
        }
    }

    /// An impulse on a dynamic body, at `point` (world) or its centre of mass.
    /// `impulse` spread over the substep as a force, at `point` (world) or
    /// the centre of mass: the solver integrates it the way it does
    /// gravity, so a drive cancelling gravity cancels it exactly.
    fn push(&mut self, body: RigidBodyHandle, impulse: Vector, point: Option<Vector>, h: Real) {
        let Some(body) = self.bodies.get_mut(body) else {
            return;
        };
        if !body.is_dynamic() {
            return;
        }
        match point {
            Some(point) => body.add_force_at_point(impulse / h, point, true),
            None => body.add_force(impulse / h, true),
        }
    }

    /// A change of spin `dw` over the substep, as a torque.
    fn spin(&mut self, body: RigidBodyHandle, dw: Vector, h: Real) {
        let Some(body) = self.bodies.get_mut(body) else {
            return;
        };
        if body.is_dynamic() {
            let torque = body.mass_properties().effective_angular_inertia() * (dw / h);
            body.add_torque(torque, true);
        }
    }

    /// Writes the read-only `Current*` properties scripts read back.
    pub(crate) fn write_constraints(&self, dom: &mut WeakDom) {
        for constraint in &self.constraints {
            let (name, value) = match constraint.drive {
                Drive::Hinge(_) => ("CurrentAngle", self.joint_position(constraint).to_degrees()),
                Drive::Prismatic(_) => ("CurrentPosition", self.joint_position(constraint)),
                Drive::Rope { .. } => ("CurrentDistance", self.end_distance(constraint)),
                Drive::Spring { .. } => ("CurrentLength", self.end_distance(constraint)),
                _ => continue,
            };
            if let Some(instance) = dom.get_mut(constraint.referent) {
                instance
                    .properties_mut()
                    .insert(name.into(), Variant::Float32(value as f32));
            }
        }
    }

    fn end_distance(&self, constraint: &Constraint) -> Real {
        (self.frame_of(&constraint.ends[1]).translation
            - self.frame_of(&constraint.ends[0]).translation)
            .length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_about_is_right_handed() {
        let a = angle_about(Vector::Z, Vector::Y, -Vector::X);
        assert!((a - std::f64::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn infinite_force_caps_to_the_largest() {
        assert_eq!(cap(Real::INFINITY), Real::MAX);
        assert_eq!(cap(-1.0), 0.0);
    }
}
