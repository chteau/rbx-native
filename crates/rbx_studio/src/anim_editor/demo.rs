//! Starting material: a walk cycle keyed on whichever of the standard R15 or
//! R6 joints a rig has, and a small locomotion graph (an idle/walk state
//! machine on a `speed` parameter) that plays it. The editor offers both as
//! examples, and scripted screenshots and tests use them as fixtures that
//! need no file.

use rbx_animation::graph::{Graph, NodeKind, Param, ParamKind, Prop, Transition, Value};
use rbx_animation::{Clip, Direction, Frame, PoseKey, Priority, Rig, Style};

/// (joint, swing about X in radians at the cycle's first key); the opposite
/// side swings the other way, and the second key mirrors the first.
const SWINGS: [(&str, f32); 12] = [
    ("LeftUpperArm", 0.6),
    ("RightUpperArm", -0.6),
    ("LeftUpperLeg", -0.5),
    ("RightUpperLeg", 0.5),
    ("LeftLowerLeg", 0.0),
    ("RightLowerLeg", 0.0),
    ("LeftLowerArm", 0.25),
    ("RightLowerArm", 0.25),
    ("Left Shoulder", 0.6),
    ("Right Shoulder", -0.6),
    ("Left Hip", -0.5),
    ("Right Hip", 0.5),
];

/// The walk's keys, on whole frames at 24, 30 and 60 fps alike.
const BEATS: [(f32, f32); 5] = [(0.0, 1.0), (0.3, 0.0), (0.6, -1.0), (0.9, 0.0), (1.2, 1.0)];

/// 1.2 seconds of walking, looped: contact, passing, contact, passing, back.
pub(crate) fn walk(rig: Option<&Rig>) -> Clip {
    let mut clip = Clip {
        name: "Walk".into(),
        looped: true,
        priority: Priority::Movement,
        keyframes: Vec::new(),
    };
    let has = |joint: &str| rig.is_none_or(|r| r.joint_for(joint).is_some());
    // R6 joints are named after the joint, R15 ones after the part; a rig's
    // own `Part1` names decide which set it gets.
    for (joint, swing) in SWINGS {
        let key_name = match rig.and_then(|r| r.joints.iter().find(|j| j.name == joint)) {
            Some(j) => rig
                .map(|r| r.part_name(j.part1).to_owned())
                .unwrap_or_default(),
            None if has(joint) => joint.to_owned(),
            None => continue,
        };
        for (time, phase) in BEATS {
            let bend = if joint.contains("Lower") {
                0.3 * (1.0 - f32::abs(phase))
            } else {
                0.0
            };
            let mut key = PoseKey::new(Frame::angles(swing * phase + bend, 0.0, 0.0));
            key.style = Style::CubicV2;
            key.direction = Direction::InOut;
            clip.set_pose(time, &key_name, key);
        }
    }
    let torso = ["UpperTorso", "LowerTorso", "Torso"]
        .into_iter()
        .find(|j| has(j));
    if let (Some(torso), Some(rig)) = (torso, rig) {
        if rig.joint_for(torso).is_some() {
            for (time, phase) in BEATS {
                let bob = 0.08 * (1.0 - f32::abs(phase));
                clip.set_pose(
                    time,
                    torso,
                    PoseKey::new(Frame::at(glam::Vec3::new(0.0, bob, 0.0))),
                );
            }
        }
    }
    if let Some(k) = clip.keyframe_index(0.3) {
        clip.keyframes[k].markers.push(rbx_animation::Marker {
            name: "Footstep".into(),
            value: "Left".into(),
        });
    }
    if let Some(k) = clip.keyframe_index(0.9) {
        clip.keyframes[k].markers.push(rbx_animation::Marker {
            name: "Footstep".into(),
            value: "Right".into(),
        });
    }
    clip
}

/// Idle ⇄ Walk on `speed`, with the walk's playback scaled by it.
pub(crate) fn locomotion(idle: &str, walk: &str) -> Graph {
    let mut g = Graph {
        name: "Locomotion".into(),
        ..Graph::default()
    };
    g.params.push(Param {
        name: "speed".into(),
        kind: ParamKind::Number,
        default: Value::Number(0.0),
    });
    let clip = |g: &mut Graph, id: &str, name: &str, at: (f32, f32)| {
        let n = g.add(NodeKind::Clip);
        g.nodes[n].name = name.into();
        g.nodes[n]
            .props
            .insert("AnimationId".into(), Prop::Value(Value::Text(id.into())));
        g.nodes[n].position = at;
        n
    };
    let idle = clip(&mut g, idle, "Idle", (-760.0, -40.0));
    let walking = clip(&mut g, walk, "Walk", (-760.0, 90.0));
    let speed = g.add(NodeKind::Speed);
    g.nodes[speed].props.insert(
        "Speed".into(),
        Prop::Expr("math.clamp(speed / 16, 0.5, 2)".into()),
    );
    g.nodes[speed].position = (-520.0, 90.0);
    let sm = g.add(NodeKind::StateMachine);
    g.nodes[sm].name = "Locomotion".into();
    g.nodes[sm].position = (-260.0, 10.0);
    g.connect(walking, speed, "Pose");
    g.connect(idle, sm, "State1");
    g.connect(speed, sm, "State2");
    let mut go = Transition::new(Some(0), 1);
    go.expression = "speed > 0.5".into();
    go.duration = 0.25;
    let mut stop = Transition::new(Some(1), 0);
    stop.expression = "speed <= 0.5".into();
    stop.duration = 0.25;
    stop.curve = Style::CubicV2;
    g.nodes[sm].transitions = vec![go, stop];
    let out = g.output().unwrap_or(0);
    g.nodes[out].position = (0.0, 10.0);
    g.connect(sm, out, "Pose");
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_animation::graph::Player;

    #[test]
    fn walk_without_a_rig_keys_every_named_joint_and_loops() {
        let clip = walk(None);
        assert!(clip.looped);
        assert_eq!(clip.length(), 1.2);
        assert!(clip.joints().contains(&"LeftUpperArm".to_owned()));
        assert_eq!(clip.keyframes[1].markers[0].name, "Footstep");
        // It starts and ends on the same pose, so the loop has no seam.
        let first = clip.pose(0.0, "LeftUpperLeg").unwrap().cframe;
        let last = clip.pose(1.2, "LeftUpperLeg").unwrap().cframe;
        assert!(first.approx_eq(&last, 1e-6));
    }

    #[test]
    fn locomotion_walks_once_speed_is_up() {
        let graph = locomotion("Idle", "Walk");
        let walk_clip = walk(None);
        let mut idle = Clip::default();
        idle.set_pose(0.0, "Head", PoseKey::new(Frame::IDENTITY));
        let resolve = |id: &str| match id {
            "Walk" => Some(walk_clip.clone()),
            "Idle" => Some(idle.clone()),
            _ => None,
        };
        let mut player = Player::new(graph, &resolve);
        assert!(!player.step(0.1).contains_key("LeftUpperArm"));
        player.set("speed", Value::Number(16.0));
        player.step(0.0);
        let pose = player.step(0.5);
        assert!(pose.contains_key("LeftUpperArm"), "walking: {pose:?}");
    }
}
