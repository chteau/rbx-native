//! `Explosion`: one put under Workspace goes off at the next physics step.
//! Every part within `BlastRadius` fires the explosion's `Hit` with its
//! distance from the centre; when there is any pressure, the joints of
//! parts within `DestroyJointRadiusPercent` of the radius break (a
//! character whose neck goes dies); loose parts get a push away from the
//! centre that grows with `BlastPressure` and the side they show it, the
//! same anywhere inside the radius. The instance goes away
//! [`LIFETIME`] seconds later. Our own model, fitted to the Explosion
//! golden.

use std::collections::HashSet;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::server::{Physics, QueryFilter, QueryShape};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{cframe, flag, vector};

/// Seconds an explosion stays in the world after going off: the golden
/// has it there after one and gone after three.
pub(crate) const LIFETIME: f64 = 2.0;

/// Explosions waiting to go off, and the ones that did with the frame
/// they went off at.
#[derive(Default)]
pub(crate) struct Explosions {
    pending: Vec<Ref>,
    done: HashSet<Ref>,
    live: Vec<(Ref, u64)>,
}

impl Explosions {
    /// Takes note of the explosions in `roots`' subtrees.
    pub(crate) fn watch(&mut self, dom: &WeakDom, roots: &[Ref]) {
        for r in roots {
            if dom.get(*r).is_some_and(|i| i.class() == "Explosion")
                && !self.done.contains(r)
                && !self.pending.contains(r)
            {
                self.pending.push(*r);
            }
        }
    }

    /// The explosions to set off at `frame`.
    pub(crate) fn take(&mut self, frame: u64) -> Vec<Ref> {
        let pending = std::mem::take(&mut self.pending);
        self.done.extend(pending.iter().copied());
        self.live.extend(pending.iter().map(|r| (*r, frame)));
        pending
    }

    /// The explosions that have had their time by `frame`.
    pub(crate) fn expired(&mut self, frame: u64, frames: u64) -> Vec<Ref> {
        let (old, live) = self
            .live
            .drain(..)
            .partition(|(_, at)| frame.saturating_sub(*at) >= frames);
        self.live = live;
        old.into_iter().map(|(r, _)| r).collect()
    }
}

/// What an explosion does.
#[derive(Default)]
pub(crate) struct Blast {
    /// Parts within the radius and their distance from the centre.
    pub(crate) hits: Vec<(Ref, f64)>,
    /// Joints to break.
    pub(crate) joints: Vec<Ref>,
    /// Pushes on loose parts.
    pub(crate) impulses: Vec<(Ref, [f64; 3])>,
}

fn number(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> f64 {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::Float32(v))) => f64::from(*v),
        Some((_, Variant::Float64(v))) => *v,
        _ => 0.0,
    }
}

/// Impulse per unit of pressure on a square stud of the side a part shows
/// the blast.
const PUSH: f64 = 1.645e-4;

/// What `explosion` does to the world as it stands.
pub(crate) fn blast(
    physics: &dyn Physics,
    dom: &WeakDom,
    db: &ReflectionDatabase,
    explosion: Ref,
) -> Blast {
    let center = vector(dom, db, explosion, "Position");
    let radius = number(dom, db, explosion, "BlastRadius").max(0.0);
    let pressure = number(dom, db, explosion, "BlastPressure").max(0.0);
    let joint_radius =
        radius * number(dom, db, explosion, "DestroyJointRadiusPercent").clamp(0.0, 1.0);
    let filter = QueryFilter {
        parts: HashSet::new(),
        include: false,
        respect_can_collide: false,
        collision_group: "Default".into(),
        max_parts: 0,
        ignore_water: true,
    };
    let at = rbx_dom::CFrameData {
        position: rbx_dom::Vector3Data {
            x: center[0] as f32,
            y: center[1] as f32,
            z: center[2] as f32,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    let mut out = Blast::default();
    let mut breaking = HashSet::new();
    for part in physics.overlap(&QueryShape::Ball(radius), &at, false, &filter) {
        let Some(frame) = cframe(dom, db, part) else {
            continue;
        };
        let p = frame.position;
        let offset = [
            f64::from(p.x) - center[0],
            f64::from(p.y) - center[1],
            f64::from(p.z) - center[2],
        ];
        let distance =
            (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt();
        out.hits.push((part, distance));
        if pressure <= 0.0 {
            continue;
        }
        if distance < joint_radius {
            breaking.insert(part);
        }
        if !flag(dom, db, part, "Anchored") && distance > 0.0 {
            let way = offset.map(|c| c / distance);
            let push =
                PUSH * pressure * shown_area(&frame.rotation, vector(dom, db, part, "Size"), way);
            out.impulses.push((part, way.map(|c| c * push)));
        }
    }
    if !breaking.is_empty() {
        out.joints = joints_holding(dom, db, &breaking);
    }
    out
}

/// The area a box of `size` turned by `rotation` shows along `way`.
fn shown_area(rotation: &[f32; 9], size: [f64; 3], way: [f64; 3]) -> f64 {
    let r = rotation.map(f64::from);
    let along =
        |axis: usize| (r[axis] * way[0] + r[3 + axis] * way[1] + r[6 + axis] * way[2]).abs();
    along(0) * size[1] * size[2] + along(1) * size[0] * size[2] + along(2) * size[0] * size[1]
}

/// The rigid joints holding any of `parts`, wherever they live.
fn joints_holding(dom: &WeakDom, db: &ReflectionDatabase, parts: &HashSet<Ref>) -> Vec<Ref> {
    let mut found = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(at) = stack.pop() {
        let Some(i) = dom.get(at) else { continue };
        stack.extend(i.children().iter().copied());
        if let Some((a, b)) = crate::physics::joint_parts(db, i) {
            if parts.contains(&a) || parts.contains(&b) {
                found.push(at);
            }
        }
    }
    found
}
