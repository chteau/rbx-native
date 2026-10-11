//! A pose flowing along a graph's wires: each joint's `Transform` with how
//! much of it applies. A joint missing from a pose is at rest (weight 0).

use std::collections::BTreeMap;

use crate::frame::Frame;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Posed {
    pub frame: Frame,
    pub weight: f32,
}

pub type Pose = BTreeMap<String, Posed>;

/// The rest pose's stand-in for a joint one side of a blend lacks.
const REST: Posed = Posed {
    frame: Frame::IDENTITY,
    weight: 0.,
};

/// Crossfades `a` into `b` by `t` (0 is all `a`).
pub fn mix(a: &Pose, b: &Pose, t: f32) -> Pose {
    let mut out = Pose::new();
    for joint in a.keys().chain(b.keys()) {
        if out.contains_key(joint) {
            continue;
        }
        let (x, y) = (
            a.get(joint).copied().unwrap_or(REST),
            b.get(joint).copied().unwrap_or(REST),
        );
        // A side that does not pose the joint should not drag the other's
        // rotation towards rest, only its weight.
        let frame = match (a.contains_key(joint), b.contains_key(joint)) {
            (true, true) => x.frame.lerp(&y.frame, t),
            (true, false) => x.frame,
            _ => y.frame,
        };
        out.insert(
            joint.clone(),
            Posed {
                frame,
                weight: x.weight + (y.weight - x.weight) * t,
            },
        );
    }
    out
}

/// A weighted blend of several poses. Weights at or below zero are left
/// out; the rest are normalised.
pub fn blend(parts: &[(Pose, f32)]) -> Pose {
    let mut out: Option<Pose> = None;
    let mut seen = 0.;
    for (pose, w) in parts.iter().filter(|(_, w)| *w > 0.) {
        seen += w;
        out = Some(match out {
            None => pose.clone(),
            Some(so_far) => mix(&so_far, pose, w / seen),
        });
    }
    out.unwrap_or_default()
}

/// `Over`: `over` laid on `base` at `weight`, joint by joint, so a joint
/// `over` leaves out (or masks off) shows `base`.
pub fn over(base: &Pose, over: &Pose, weight: f32) -> Pose {
    let mut out = base.clone();
    for (joint, top) in over {
        let under = base.get(joint).copied().unwrap_or(REST);
        let t = (top.weight * weight).clamp(0., 1.);
        let frame = if base.contains_key(joint) {
            under.frame.lerp(&top.frame, t)
        } else {
            top.frame
        };
        out.insert(
            joint.clone(),
            Posed {
                frame,
                weight: under.weight + (1. - under.weight) * t,
            },
        );
    }
    out
}

/// `Add`: `additive` applied on top of `base`, scaled by `weight`.
pub fn add(base: &Pose, additive: &Pose, weight: f32) -> Pose {
    let mut out = base.clone();
    for (joint, delta) in additive {
        let scaled = Frame::IDENTITY.lerp(&delta.frame, weight * delta.weight);
        let under = base.get(joint).copied().unwrap_or(Posed {
            frame: Frame::IDENTITY,
            weight: 1.,
        });
        out.insert(
            joint.clone(),
            Posed {
                frame: under.frame.mul(&scaled),
                weight: under.weight.max(1.),
            },
        );
    }
    out
}

/// `Subtract`: `a − weight·b`, which turns a clip into an additive one.
pub fn subtract(a: &Pose, b: &Pose, weight: f32) -> Pose {
    let mut out = a.clone();
    for (joint, minus) in b {
        let scaled = Frame::IDENTITY.lerp(&minus.frame, weight * minus.weight);
        let from = a.get(joint).copied().unwrap_or(Posed {
            frame: Frame::IDENTITY,
            weight: 1.,
        });
        out.insert(
            joint.clone(),
            Posed {
                frame: from.frame.mul(&scaled.inverse()),
                weight: from.weight.max(1.),
            },
        );
    }
    out
}

/// Every joint's final `Transform`: its pose scaled by its weight from rest.
pub fn resolve(pose: &Pose) -> BTreeMap<String, Frame> {
    pose.iter()
        .filter(|(_, p)| p.weight > 0.)
        .map(|(j, p)| (j.clone(), Frame::IDENTITY.lerp(&p.frame, p.weight.min(1.))))
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn at(joint: &str, x: f32) -> Pose {
        Pose::from([(
            joint.to_owned(),
            Posed {
                frame: Frame::at(Vec3::new(x, 0., 0.)),
                weight: 1.,
            },
        )])
    }

    fn x(p: &Pose, joint: &str) -> f32 {
        p[joint].frame.position.x
    }

    #[test]
    fn mix_keeps_one_sided_joints_in_place() {
        let m = mix(&at("Head", 2.), &at("Torso", 4.), 0.25);
        assert_eq!(x(&m, "Head"), 2.);
        assert!((m["Head"].weight - 0.75).abs() < 1e-6);
        assert!((m["Torso"].weight - 0.25).abs() < 1e-6);
        let both = mix(&at("Head", 0.), &at("Head", 4.), 0.5);
        assert!((x(&both, "Head") - 2.).abs() < 1e-6);
    }

    #[test]
    fn blend_weights() {
        let b = blend(&[(at("Head", 0.), 0.25), (at("Head", 4.), 0.75)]);
        assert!((x(&b, "Head") - 3.).abs() < 1e-5);
        assert!(blend(&[]).is_empty());
    }

    #[test]
    fn over_add_subtract() {
        let o = over(&at("Head", 0.), &at("Head", 4.), 0.5);
        assert!((x(&o, "Head") - 2.).abs() < 1e-5);
        let a = add(&at("Head", 1.), &at("Head", 2.), 0.5);
        assert!((x(&a, "Head") - 2.).abs() < 1e-5);
        let s = subtract(&at("Head", 3.), &at("Head", 2.), 1.);
        assert!((x(&s, "Head") - 1.).abs() < 1e-5);
    }

    #[test]
    fn resolve_scales_from_rest() {
        let mut p = at("Head", 4.);
        p.get_mut("Head").unwrap().weight = 0.5;
        p.insert("Torso".into(), REST);
        let r = resolve(&p);
        assert!((r["Head"].position.x - 2.).abs() < 1e-5);
        assert!(!r.contains_key("Torso"));
    }
}
