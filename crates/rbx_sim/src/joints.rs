//! What a script can ask a part about its joints and its assembly:
//! `GetJoints`, `GetConnectedParts`, `GetRootPart` / `AssemblyRootPart`,
//! `GetNoCollisionConstraints` and `GetTouchingParts`, read from the DOM as
//! it stands (a weld made this frame counts at once), as the creator docs
//! describe them.

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
];

fn is_part(class: &str) -> bool {
    ReflectionDatabase::shared().is_subclass_of(class, "BasePart")
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    METHODS.contains(&key) && is_part(class)
}

pub(crate) fn is_property(class: &str, key: &str) -> bool {
    key == "AssemblyRootPart" && is_part(class)
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
        let Some((a, b)) = dom.get(r).and_then(|i| crate::physics::joint_parts(db, i)) else {
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
) -> CallReply {
    let list = match method {
        "GetJoints" => joints_of(dom, db, part, false),
        "GetNoCollisionConstraints" => joints_of(dom, db, part, true),
        "GetConnectedParts" => {
            connected(dom, db, part, matches!(args.first(), Some(Arg::Bool(true))))
        }
        "GetTouchingParts" => touching(physics, dom, db, part),
        _ => return CallReply::value(Arg::Instance(root_part(dom, db, part))),
    };
    CallReply::value(Arg::Instances(list))
}
