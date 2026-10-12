//! Moving a clip between its `KeyframeSequence` form ([`Clip`]) and its
//! curve form ([`CurveClip`]), as the editor does when the Curve Editor is
//! first opened and when the clip is saved.
//!
//! Pose easing maps onto curve keys like this:
//!
//! - `Linear` → `Linear`.
//! - `Constant` → `Constant`. `Out` holds the earlier pose, which is exactly
//!   a Constant key. `InOut` swaps at the midpoint, so a Constant key holding
//!   the next pose goes there. `In` jumps to the next pose straight away; a
//!   Constant key holding it goes [`STEP`] after the pose.
//! - `CubicV2` → `Cubic`. Both one-sided eases are single cubics, so Hermite
//!   tangents reproduce them exactly: `In` (u³) leaves with slope 0 and
//!   arrives with slope 3Δ/h, and `Out` is its mirror. `InOut` is two cubics
//!   joined at the midpoint, so it gets an extra Cubic key there, with
//!   slope 3Δ/h on both sides and 0 at the ends. The deprecated `Cubic` is
//!   `CubicV2` with its direction reversed, as [`ease`] plays it.
//! - `Elastic` and `Bounce` have no curve equivalent: the editor's
//!   Generate Curve bakes them into Linear keys at the frame rate.
//!
//! Rotation is converted to Euler angles key by key, each triple unwrapped
//! against the one before it. Euler channels interpolate independently, so
//! a segment that turns about more than one axis at once moves along a
//! slightly different path than the `KeyframeSequence`'s slerp; that is the
//! same loss the editor's own conversion accepts. A pose's `Weight` has no
//! curve counterpart and is dropped (it comes back as 1).

use glam::Vec3;

use super::{bake_times, Channel, CurveClip, FloatCurve, FloatKey, Interpolation};
use super::{JointCurves, RotationOrder};
use crate::clip::{Clip, PoseKey, TIME_EPSILON};
use crate::easing::{ease, Direction, Style};
use crate::frame::Frame;

/// How long after a `Constant`/`In` pose the next pose takes over: well
/// under a frame, but far enough above [`TIME_EPSILON`] that the step key
/// stays a key of its own through a round trip.
pub(crate) const STEP: f32 = 1e-3;

/// The six channel values at a key, in [`Channel`] order.
type Values = [f32; 6];

struct KeySpec {
    time: f32,
    values: Values,
    interpolation: Interpolation,
    left: Option<Values>,
    right: Option<Values>,
}

impl KeySpec {
    fn new(time: f32, values: Values, interpolation: Interpolation) -> KeySpec {
        KeySpec {
            time,
            values,
            interpolation,
            left: None,
            right: None,
        }
    }
}

fn euler_of(values: &Values) -> Vec3 {
    Vec3::new(values[3], values[4], values[5])
}

fn values_of(frame: &Frame, order: RotationOrder, near: Vec3) -> Values {
    let e = order.unwrap(order.decompose(frame.rotation), near);
    let p = frame.position;
    [p.x, p.y, p.z, e.x, e.y, e.z]
}

fn mix(a: &Values, b: &Values, f: impl Fn(f32, f32) -> f32) -> Values {
    std::array::from_fn(|i| f(a[i], b[i]))
}

/// [`from_clip_in_order`] with the editor's default Euler order, XYZ.
pub fn from_clip(clip: &Clip, fps: f32) -> CurveClip {
    from_clip_in_order(clip, fps, RotationOrder::XYZ)
}

/// `fps` is the rate Elastic and Bounce segments are baked at.
pub fn from_clip_in_order(clip: &Clip, fps: f32, order: RotationOrder) -> CurveClip {
    let mut curves = CurveClip {
        name: clip.name.clone(),
        looped: clip.looped,
        priority: clip.priority,
        ..CurveClip::default()
    };
    for joint in clip.joints() {
        let track: Vec<(f32, PoseKey)> = clip.track(&joint).map(|(t, k)| (t, *k)).collect();
        curves
            .joints
            .insert(joint, joint_curves(&track, fps, order));
    }
    for keyframe in &clip.keyframes {
        for marker in &keyframe.markers {
            curves.markers.push((keyframe.time, marker.clone()));
        }
    }
    curves
}

fn joint_curves(track: &[(f32, PoseKey)], fps: f32, order: RotationOrder) -> JointCurves {
    let mut specs: Vec<KeySpec> = Vec::new();
    if let Some((t, first)) = track.first() {
        // Starting from the solution nearest no turn at all keeps a rig's
        // rest pose reading as zeros rather than as an equivalent flip.
        let values = values_of(&first.cframe, order, Vec3::ZERO);
        specs.push(KeySpec::new(*t, values, Interpolation::Linear));
    }
    for pair in track.windows(2) {
        segment(&mut specs, pair[0], pair[1], fps, order);
    }
    let mut joint = JointCurves {
        order,
        ..JointCurves::default()
    };
    for channel in Channel::ALL {
        let c = channel.index();
        *joint.curve_mut(channel) = FloatCurve {
            keys: specs
                .iter()
                .map(|s| FloatKey {
                    time: s.time,
                    value: s.values[c],
                    interpolation: s.interpolation,
                    left_tangent: s.left.map(|l| l[c]),
                    right_tangent: s.right.map(|r| r[c]),
                })
                .collect(),
        };
    }
    joint
}

/// Appends the keys for the way from `from` to `to`; the last spec is
/// `from`'s own key, whose outgoing mode this sets.
fn segment(
    specs: &mut Vec<KeySpec>,
    (t0, from): (f32, PoseKey),
    (t1, to): (f32, PoseKey),
    fps: f32,
    order: RotationOrder,
) {
    let h = t1 - t0;
    let v0 = specs.last().map(|s| s.values).unwrap_or_default();
    // Unwrapping against the earlier key picks the short way round, the
    // same arc the pose slerp takes.
    let v1 = values_of(&to.cframe, order, euler_of(&v0));
    let direction = match (from.style, from.direction) {
        (Style::Cubic, Direction::In) => Direction::Out,
        (Style::Cubic, Direction::Out) => Direction::In,
        (_, d) => d,
    };
    let last = specs.len() - 1;
    let mut end = KeySpec::new(t1, v1, Interpolation::Linear);
    match from.style {
        Style::Linear => {}
        Style::Constant => {
            specs[last].interpolation = Interpolation::Constant;
            let step = match direction {
                Direction::Out => None,
                Direction::InOut => Some(h / 2.),
                Direction::In => Some(STEP.min(h / 2.)),
            };
            if let Some(step) = step {
                specs.push(KeySpec::new(t0 + step, v1, Interpolation::Constant));
            }
        }
        Style::Cubic | Style::CubicV2 => {
            specs[last].interpolation = Interpolation::Cubic;
            let zero = [0.; 6];
            let slope = mix(&v0, &v1, |a, b| 3. * (b - a) / h);
            match direction {
                Direction::In => {
                    specs[last].right = Some(zero);
                    end.left = Some(slope);
                }
                Direction::Out => {
                    specs[last].right = Some(slope);
                    end.left = Some(zero);
                }
                Direction::InOut => {
                    specs[last].right = Some(zero);
                    let mid = mix(&v0, &v1, |a, b| (a + b) / 2.);
                    let mut key = KeySpec::new(t0 + h / 2., mid, Interpolation::Cubic);
                    key.left = Some(slope);
                    key.right = Some(slope);
                    specs.push(key);
                    end.left = Some(zero);
                }
            }
        }
        Style::Elastic | Style::Bounce => {
            for t in bake_times(t0, t1, fps) {
                let alpha = ease(from.style, from.direction, (t - t0) / h);
                let frame = from.cframe.lerp(&to.cframe, alpha);
                let near = euler_of(&specs[specs.len() - 1].values);
                let values = values_of(&frame, order, near);
                specs.push(KeySpec::new(t, values, Interpolation::Linear));
            }
        }
    }
    specs.push(end);
}

/// Bakes curves back into poses. Every joint gets a pose at each of its
/// curve keys (the union over its six channels, since a pose is a whole
/// CFrame). A segment a pose's own easing reproduces stays one segment; any
/// other (cubic, or mixing modes across channels, or an Euler path that
/// strays from the slerp) is baked at `fps` with Linear poses.
pub fn to_clip(curves: &CurveClip, fps: f32) -> Clip {
    let mut clip = Clip {
        name: curves.name.clone(),
        looped: curves.looped,
        priority: curves.priority,
        keyframes: Vec::new(),
    };
    for (name, joint) in &curves.joints {
        let times = key_times(joint);
        for (i, &ta) in times.iter().enumerate() {
            let mut key = PoseKey::new(joint.eval(ta));
            let Some(&tb) = times.get(i + 1) else {
                clip.set_pose(ta, name, key);
                break;
            };
            match pose_style(joint, ta, tb) {
                Some(style) => {
                    key.style = style;
                    // Out is the Constant reading that holds the earlier pose.
                    key.direction = Direction::Out;
                    clip.set_pose(ta, name, key);
                }
                None => {
                    clip.set_pose(ta, name, key);
                    for t in bake_times(ta, tb, fps) {
                        clip.set_pose(t, name, PoseKey::new(joint.eval(t)));
                    }
                }
            }
        }
    }
    for (time, marker) in &curves.markers {
        clip.keyframe_mut(*time).markers.push(marker.clone());
    }
    clip
}

fn key_times(joint: &JointCurves) -> Vec<f32> {
    let mut times: Vec<f32> = Channel::ALL
        .iter()
        .flat_map(|&c| joint.curve(c).keys.iter().map(|k| k.time))
        .collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|b, a| (*b - *a).abs() < TIME_EPSILON);
    times
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Flat,
    Step,
    Linear,
    Cubic,
}

/// What one channel does between two consecutive joint key times.
fn kind(curve: &FloatCurve, ta: f32, tb: f32) -> Kind {
    let i = curve.keys.partition_point(|k| k.time <= ta + TIME_EPSILON);
    if i == 0 || i == curve.keys.len() {
        return Kind::Flat;
    }
    match curve.keys[i - 1].interpolation {
        Interpolation::Linear => Kind::Linear,
        Interpolation::Cubic => Kind::Cubic,
        Interpolation::Constant => {
            let (a, b) = (curve.eval(ta), curve.eval(tb));
            if a.zip(b).is_some_and(|(a, b)| (a - b).abs() < 1e-6) {
                Kind::Flat
            } else {
                Kind::Step
            }
        }
    }
}

/// The pose easing that reproduces the joint between `ta` and `tb`, or
/// `None` when the segment has to be baked.
fn pose_style(joint: &JointCurves, ta: f32, tb: f32) -> Option<Style> {
    let kinds: Vec<Kind> = Channel::ALL
        .iter()
        .map(|&c| kind(joint.curve(c), ta, tb))
        .collect();
    if kinds.contains(&Kind::Cubic) {
        return None;
    }
    if kinds.contains(&Kind::Step) {
        return kinds
            .iter()
            .all(|k| matches!(k, Kind::Flat | Kind::Step))
            .then_some(Style::Constant);
    }
    let (a, b) = (joint.eval(ta), joint.eval(tb));
    let follows_slerp = [0.25, 0.5, 0.75].iter().all(|&u| {
        let curve = joint.eval(ta + (tb - ta) * u);
        let pose = a.lerp(&b, u);
        curve.position.abs_diff_eq(pose.position, 1e-4)
            && curve.rotation.angle_between(pose.rotation) < 1e-3
    });
    follows_slerp.then_some(Style::Linear)
}
