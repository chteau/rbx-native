//! What a script can ask a part about its joints and its assembly:
//! `GetJoints`, `GetConnectedParts`, `GetRootPart` / `AssemblyRootPart`,
//! `GetNoCollisionConstraints` and `GetTouchingParts`, read from the DOM as
//! it stands (a weld made this frame counts at once), as the creator docs
//! describe them; and what follows from the assembly's mass: its centre,
//! the velocity at a point, impulses off centre, torque and angular
//! acceleration, `IsGrounded`, plus `Attachment:GetConstraints`.

use rapier3d::prelude::{Mat3, Vector};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Physics, QueryFilter, QueryShape};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{cframe, flag, reference, vector};

const METHODS: &[&str] = &[
    "GetJoints",
    "GetConnectedParts",
    "GetRootPart",
    "GetNoCollisionConstraints",
    "GetTouchingParts",
    "GetVelocityAtPosition",
    "ApplyImpulseAtPosition",
    "IsGrounded",
    "AngularAccelerationToTorque",
    "TorqueToAngularAcceleration",
];

const PROPERTIES: &[&str] = &["AssemblyRootPart", "AssemblyCenterOfMass", "CenterOfMass"];

fn is_part(class: &str) -> bool {
    ReflectionDatabase::shared().is_subclass_of(class, "BasePart")
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    (METHODS.contains(&key) && is_part(class))
        || (key == "GetConstraints"
            && ReflectionDatabase::shared().is_subclass_of(class, "Attachment"))
}

pub(crate) fn is_property(class: &str, key: &str) -> bool {
    PROPERTIES.contains(&key) && is_part(class)
}

/// `AssemblyRootPart`, `AssemblyCenterOfMass` (world space; an anchored
/// part's own, as each anchored part answers alone) and `CenterOfMass`
/// (the part's own, in its own space).
pub(crate) fn property(dom: &WeakDom, db: &ReflectionDatabase, part: Ref, key: &str) -> Arg {
    if key == "AssemblyRootPart" {
        return Arg::Instance(root_part(dom, db, part));
    }
    let (assembly, anchored) = crate::ownership::assembly_in_dom(dom, db, part);
    let parts = if key == "CenterOfMass" || anchored {
        vec![part]
    } else {
        assembly
    };
    let frame = crate::humanoid::cframe(dom, db, part);
    let position = frame.map_or(Vector::ZERO, |f| {
        Vector::new(
            f.position.x.into(),
            f.position.y.into(),
            f.position.z.into(),
        )
    });
    let center = crate::physics::mass_props(dom, db, &parts).map_or(position, |m| m.center);
    let center = match (key, frame) {
        ("CenterOfMass", Some(frame)) => {
            let r = frame.rotation.map(f64::from);
            let d = center - position;
            Vector::new(
                r[0] * d.x + r[3] * d.y + r[6] * d.z,
                r[1] * d.x + r[4] * d.y + r[7] * d.z,
                r[2] * d.x + r[5] * d.y + r[8] * d.z,
            )
        }
        _ => center,
    };
    Arg::Vector3(center.to_array().map(|v| v as f32))
}

fn vec_arg(v: Vector) -> CallReply {
    CallReply::value(Arg::Vector3(v.to_array().map(|c| c as f32)))
}

fn vec_of(arg: Option<&Arg>) -> Option<Vector> {
    match arg {
        Some(Arg::Vector3(v)) => Some(Vector::new(v[0].into(), v[1].into(), v[2].into())),
        _ => None,
    }
}

/// The assembly's mass properties, `None` when it is anchored.
fn free_mass(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    part: Ref,
) -> Option<crate::physics::MassProps> {
    let (parts, anchored) = crate::ownership::assembly_in_dom(dom, db, part);
    if anchored {
        return None;
    }
    crate::physics::mass_props(dom, db, &parts)
}

fn assembly_call(
    physics: &dyn Physics,
    dom: &WeakDom,
    db: &ReflectionDatabase,
    part: Ref,
    method: &str,
    args: &[Arg],
) -> Result<CallReply, String> {
    let first = vec_of(args.first());
    let second = vec_of(args.get(1));
    let missing = |n: usize| format!("Argument {n} missing or nil");
    match method {
        // Rigid joints only: a hinge or a rope to an anchored part doesn't
        // ground it.
        "IsGrounded" => {
            let grounded = crate::ownership::assembly_in_dom(dom, db, part).1;
            Ok(CallReply::value(Arg::Bool(grounded)))
        }
        // From the velocities the assembly holds, anchored or not.
        "GetVelocityAtPosition" => {
            let at = first.ok_or_else(|| missing(1))?;
            let parts = crate::ownership::assembly_in_dom(dom, db, part).0;
            let Some(mass) = crate::physics::mass_props(dom, db, &parts) else {
                return Ok(vec_arg(Vector::ZERO));
            };
            let v = Vector::from(vector(dom, db, part, "AssemblyLinearVelocity"));
            let w = Vector::from(vector(dom, db, part, "AssemblyAngularVelocity"));
            Ok(vec_arg(v + w.cross(at - mass.center)))
        }
        "ApplyImpulseAtPosition" => {
            let impulse = first.ok_or_else(|| missing(1))?;
            let at = second.ok_or_else(|| missing(2))?;
            if let Some(mass) = free_mass(dom, db, part) {
                physics.apply_impulse(part, impulse.to_array(), false);
                let spin = (at - mass.center).cross(impulse);
                physics.apply_impulse(part, spin.to_array(), true);
            }
            Ok(CallReply::default())
        }
        _ => {
            let given = first.ok_or_else(|| missing(1))?;
            let spin = second.unwrap_or(Vector::ZERO);
            let Some(mass) = free_mass(dom, db, part) else {
                return Ok(vec_arg(Vector::ZERO));
            };
            let i: Mat3 = mass.inertia;
            let gyro = spin.cross(i * spin);
            Ok(vec_arg(if method == "AngularAccelerationToTorque" {
                i * given + gyro
            } else {
                i.inverse() * (given - gyro)
            }))
        }
    }
}

/// `Attachment:GetConstraints()`: the constraints using it.
fn constraints_of(dom: &WeakDom, db: &ReflectionDatabase, attachment: Ref) -> Vec<Ref> {
    every(dom)
        .into_iter()
        .filter(|r| {
            dom.get(*r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "Constraint"))
                && (reference(dom, *r, "Attachment0") == Some(attachment)
                    || reference(dom, *r, "Attachment1") == Some(attachment))
        })
        .collect()
}

fn every(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(at) = stack.pop() {
        if let Some(i) = dom.get(at) {
            stack.extend(i.children().iter().copied());
            out.push(at);
        }
    }
    out
}

/// The parts a joint or constraint holds, through `Part0`/`Part1` or its
/// attachments' parents.
fn held(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> Option<[Option<Ref>; 2]> {
    let class = dom.get(r)?.class();
    if db.is_subclass_of(class, "JointInstance")
        || matches!(class, "WeldConstraint" | "NoCollisionConstraint")
    {
        let part = |name: &str| {
            reference(dom, r, name).or_else(|| reference(dom, r, &format!("{name}Internal")))
        };
        return Some([part("Part0"), part("Part1")]);
    }
    if db.is_subclass_of(class, "Constraint") {
        let parent = |name| reference(dom, r, name).and_then(|a| dom.parent(a));
        return Some([parent("Attachment0"), parent("Attachment1")]);
    }
    None
}

fn joints_of(dom: &WeakDom, db: &ReflectionDatabase, part: Ref, no_collision: bool) -> Vec<Ref> {
    every(dom)
        .into_iter()
        .filter(|r| {
            let is_nc = dom
                .get(*r)
                .is_some_and(|i| i.class() == "NoCollisionConstraint");
            is_nc == no_collision
                && held(dom, db, *r).is_some_and(|parts| parts.contains(&Some(part)))
        })
        .collect()
}

/// Parts joined to `part` by rigid joints: the ones touching it directly,
/// or (`recursive`) its whole assembly. `part` itself comes first.
fn connected(dom: &WeakDom, db: &ReflectionDatabase, part: Ref, recursive: bool) -> Vec<Ref> {
    if recursive {
        return crate::ownership::assembly_in_dom(dom, db, part).0;
    }
    let mut out = vec![part];
    for r in every(dom) {
        let Some((a, b)) = dom
            .get(r)
            .and_then(|i| crate::physics::joint_parts(dom, db, i))
        else {
            continue;
        };
        let other = if a == part {
            b
        } else if b == part {
            a
        } else {
            continue;
        };
        if dom.get(other).is_some() && !out.contains(&other) {
            out.push(other);
        }
    }
    out
}

/// The assembly's root: an anchored part, else a part with mass over a
/// massless one, then the higher `RootPriority`, then a
/// `HumanoidRootPart`, then the biggest.
pub(crate) fn root_part(dom: &WeakDom, db: &ReflectionDatabase, part: Ref) -> Ref {
    let parts = crate::ownership::assembly_in_dom(dom, db, part).0;
    let key = |r: &Ref| {
        let priority = match dom
            .get(*r)
            .and_then(|i| db.stored_or_default(i, "RootPriority"))
        {
            Some((_, Variant::Int32(p))) => *p,
            _ => 0,
        };
        let size = vector(dom, db, *r, "Size");
        let volume = size[0] * size[1] * size[2];
        let named = dom.get(*r).is_some_and(|i| i.name() == "HumanoidRootPart");
        (
            flag(dom, db, *r, "Anchored"),
            !flag(dom, db, *r, "Massless"),
            priority,
            named,
            (volume * 1000.0) as i64,
        )
    };
    parts.into_iter().max_by_key(key).unwrap_or(part)
}

/// Parts in contact with `part`: overlapping it, or resting against a block
/// within a hundredth of a stud. A part that doesn't collide and has no
/// `TouchInterest` touches nothing; the others count when they collide or
/// listen for touches.
fn touching(physics: &dyn Physics, dom: &WeakDom, db: &ReflectionDatabase, part: Ref) -> Vec<Ref> {
    let listens = |r: Ref| {
        dom.get(r).is_some_and(|i| {
            i.children()
                .iter()
                .any(|c| dom.get(*c).is_some_and(|c| c.class() == "TouchTransmitter"))
        })
    };
    if !flag(dom, db, part, "CanCollide") && !listens(part) {
        return Vec::new();
    }
    let Some(at) = cframe(dom, db, part) else {
        return Vec::new();
    };
    let filter = QueryFilter {
        parts: [part].into_iter().collect(),
        include: false,
        respect_can_collide: false,
        collision_group: "Default".into(),
        max_parts: 0,
        ignore_water: true,
    };
    let mut found = physics.overlap(&QueryShape::Part(part), &at, true, &filter);
    // PartType.Block is 1.
    let block = dom.get(part).is_some_and(|i| {
        matches!(
            i.class(),
            "Part" | "SpawnLocation" | "Seat" | "VehicleSeat" | "SkateboardPlatform" | "TrussPart"
        ) && !matches!(db.stored_or_default(i, "Shape"), Some((_, Variant::Enum(s))) if *s != 1)
    });
    if block {
        let size = vector(dom, db, part, "Size");
        let grown = QueryShape::Box([size[0] + 0.02, size[1] + 0.02, size[2] + 0.02]);
        for r in physics.overlap(&grown, &at, true, &filter) {
            if !found.contains(&r) {
                found.push(r);
            }
        }
    }
    found.retain(|r| flag(dom, db, *r, "CanCollide") || listens(*r));
    found
}

pub(crate) fn call(
    physics: &dyn Physics,
    dom: &WeakDom,
    db: &ReflectionDatabase,
    part: Ref,
    method: &str,
    args: &[Arg],
) -> Result<CallReply, String> {
    if method == "GetConstraints" {
        return Ok(CallReply::value(Arg::Instances(constraints_of(
            dom, db, part,
        ))));
    }
    if !matches!(
        method,
        "GetJoints"
            | "GetNoCollisionConstraints"
            | "GetConnectedParts"
            | "GetTouchingParts"
            | "GetRootPart"
    ) {
        return assembly_call(physics, dom, db, part, method, args);
    }
    let list = match method {
        "GetJoints" => joints_of(dom, db, part, false),
        "GetNoCollisionConstraints" => joints_of(dom, db, part, true),
        "GetConnectedParts" => {
            connected(dom, db, part, matches!(args.first(), Some(Arg::Bool(true))))
        }
        "GetTouchingParts" => touching(physics, dom, db, part),
        _ => return Ok(CallReply::value(Arg::Instance(root_part(dom, db, part)))),
    };
    Ok(CallReply::value(Arg::Instances(list)))
}
