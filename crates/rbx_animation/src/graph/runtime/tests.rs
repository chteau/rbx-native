use glam::Vec3;

use super::*;
use crate::clip::PoseKey;
use crate::graph::model::{Mask, MaskJoint, Param, Transition, WaitFor};

mod semantics;

/// A one-second clip holding `joint` at x = `x` (and at `x + 1` at its end,
/// so its time can be read off the pose).
fn ramp(joint: &str, x: f32) -> Clip {
    stretch(joint, x, 1.)
}

/// `ramp` over `length` seconds, x still rising by one per second.
fn stretch(joint: &str, x: f32, length: f32) -> Clip {
    let mut clip = Clip::default();
    clip.set_pose(0., joint, PoseKey::new(Frame::at(Vec3::new(x, 0., 0.))));
    clip.set_pose(
        length,
        joint,
        PoseKey::new(Frame::at(Vec3::new(x + length, 0., 0.))),
    );
    clip
}

fn resolve(id: &str) -> Option<Clip> {
    match id {
        "idle" => Some(ramp("Torso", 0.)),
        "run" => Some(ramp("Torso", 10.)),
        "wave" => Some(ramp("Arm", 20.)),
        "jog" => Some(ramp("Torso", 50.)),
        "long" => Some(stretch("Leg", 30., 2.)),
        _ => None,
    }
}

fn clip_node(g: &mut Graph, id: &str, mode: &str) -> usize {
    let n = g.add(NodeKind::Clip);
    g.nodes[n]
        .props
        .insert("AnimationId".into(), Prop::Value(Value::Text(id.into())));
    g.nodes[n]
        .props
        .insert("PlayMode".into(), Prop::Value(Value::Text(mode.into())));
    n
}

fn x(pose: &BTreeMap<String, Frame>, joint: &str) -> f32 {
    pose[joint].position.x
}

#[test]
fn a_clip_loops_and_speed_scales_time() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "idle", "Loop");
    let speed = g.add(NodeKind::Speed);
    g.nodes[speed]
        .props
        .insert("Speed".into(), Prop::Value(Value::Number(2.)));
    g.connect(clip, speed, "Pose");
    g.connect(speed, g.output().unwrap(), "Pose");
    let mut p = Player::new(g, &resolve);
    assert!((x(&p.step(0.25), "Torso") - 0.5).abs() < 1e-4);
    assert!((x(&p.step(0.375), "Torso") - 0.25).abs() < 1e-4, "wrapped");
}

#[test]
fn state_machine_crossfades_on_an_expression() {
    let mut g = Graph::default();
    g.params.push(Param {
        name: "speed".into(),
        kind: ParamKind::Number,
        default: Value::Number(0.),
    });
    let idle = clip_node(&mut g, "idle", "Loop");
    let run = clip_node(&mut g, "run", "Loop");
    let sm = g.add(NodeKind::StateMachine);
    g.connect(idle, sm, "State1");
    g.connect(run, sm, "State2");
    let mut t = Transition::new(Some(0), 1);
    t.expression = "speed > 5".into();
    t.duration = 0.5;
    g.nodes[sm].transitions.push(t);
    g.connect(sm, g.output().unwrap(), "Pose");

    let mut p = Player::new(g, &resolve);
    assert!(x(&p.step(0.), "Torso") < 1.);
    p.set("speed", Value::Number(9.));
    p.step(0.);
    assert_eq!(p.active(sm), Some(1));
    // Halfway through the fade: idle at 0.25, run restarted at 10.25.
    let mid = x(&p.step(0.25), "Torso");
    assert!((mid - 5.25).abs() < 1e-3, "{mid}");
    let done = x(&p.step(0.25), "Torso");
    assert!((done - 10.5).abs() < 1e-3, "{done}");
}

#[test]
fn any_state_transitions_and_finished_transitions() {
    let mut g = Graph::default();
    g.params.push(Param {
        name: "jump".into(),
        kind: ParamKind::Trigger,
        default: Value::Bool(false),
    });
    let idle = clip_node(&mut g, "idle", "Loop");
    let wave = clip_node(&mut g, "wave", "OnceAndHold");
    let sm = g.add(NodeKind::StateMachine);
    g.connect(idle, sm, "State1");
    g.connect(wave, sm, "State2");
    let mut go = Transition::new(None, 1);
    go.expression = "jump".into();
    go.duration = 0.;
    let mut back = Transition::new(Some(1), 0);
    back.wait_for = WaitFor::Finished;
    back.duration = 0.;
    g.nodes[sm].transitions = vec![go, back];
    g.connect(sm, g.output().unwrap(), "Pose");

    let mut p = Player::new(g, &resolve);
    p.step(0.1);
    p.fire("jump");
    p.step(0.);
    assert_eq!(p.active(sm), Some(1));
    assert_eq!(
        p.get("jump"),
        Some(&Value::Bool(false)),
        "triggers reset after a step"
    );
    p.step(0.5);
    assert_eq!(p.active(sm), Some(1));
    p.step(0.6);
    assert_eq!(p.active(sm), Some(0), "back once the wave finished");
}

#[test]
fn select_follows_a_parameter() {
    let mut g = Graph::default();
    g.params.push(Param {
        name: "pick".into(),
        kind: ParamKind::Text,
        default: Value::Text("Input1".into()),
    });
    let idle = clip_node(&mut g, "idle", "Loop");
    let run = clip_node(&mut g, "run", "Loop");
    let sel = g.add(NodeKind::Select);
    g.nodes[sel]
        .props
        .insert("Selection".into(), Prop::Param("pick".into()));
    g.nodes[sel].props.insert(
        "DefaultTransitionDuration".into(),
        Prop::Value(Value::Number(0.)),
    );
    g.connect(idle, sel, "Input1");
    g.connect(run, sel, "Input2");
    g.connect(sel, g.output().unwrap(), "Pose");
    let mut p = Player::new(g, &resolve);
    assert!(x(&p.step(0.), "Torso") < 1.);
    p.set("pick", Value::Text("Input2".into()));
    assert!(x(&p.step(0.), "Torso") >= 10.);
}

#[test]
fn blend1d_interpolates_and_extrapolates() {
    let mut g = Graph::default();
    let idle = clip_node(&mut g, "idle", "Loop");
    let run = clip_node(&mut g, "run", "Loop");
    let b = g.add(NodeKind::Blend1D);
    g.connect(idle, b, "A");
    g.connect(run, b, "B");
    g.nodes[b].inputs[1]
        .props
        .insert("Position".into(), Prop::Value(Value::Number(10.)));
    g.nodes[b]
        .props
        .insert("Position".into(), Prop::Expr("2 + 0.5".into()));
    g.connect(b, g.output().unwrap(), "Pose");
    let mut p = Player::new(g.clone(), &resolve);
    assert!((x(&p.step(0.), "Torso") - 2.5).abs() < 1e-4);
    g.nodes[b]
        .props
        .insert("Position".into(), Prop::Value(Value::Number(15.)));
    let mut p = Player::new(g, &resolve);
    assert!((x(&p.step(0.), "Torso") - 15.).abs() < 1e-4);
}

#[test]
fn one_shot_overlays_then_fades_out() {
    let mut g = Graph::default();
    g.params.push(Param {
        name: "wave".into(),
        kind: ParamKind::Trigger,
        default: Value::Bool(false),
    });
    let idle = clip_node(&mut g, "idle", "Loop");
    let wave = clip_node(&mut g, "wave", "OnceAndHold");
    let shot = g.add(NodeKind::OneShot);
    g.nodes[shot]
        .props
        .insert("Trigger".into(), Prop::Param("wave".into()));
    g.nodes[shot].props.insert(
        "TransitionInDuration".into(),
        Prop::Value(Value::Number(0.)),
    );
    g.connect(idle, shot, "Base");
    g.connect(wave, shot, "OneShot");
    g.connect(shot, g.output().unwrap(), "Pose");
    let mut p = Player::new(g, &resolve);
    assert!(!p.step(0.1).contains_key("Arm"));
    p.fire("wave");
    let during = p.step(0.1);
    assert!(during.contains_key("Arm") && during.contains_key("Torso"));
    p.step(1.0);
    p.step(0.1);
    assert!(
        !p.step(0.1).contains_key("Arm"),
        "faded out after the shot ended"
    );
}

#[test]
fn sequence_plays_inputs_in_order_and_mask_hides_joints() {
    let mut g = Graph::default();
    let idle = clip_node(&mut g, "idle", "OnceAndHold");
    let wave = clip_node(&mut g, "wave", "OnceAndHold");
    let seq = g.add(NodeKind::Sequence);
    g.nodes[seq].props.insert(
        "DefaultTransitionDuration".into(),
        Prop::Value(Value::Number(0.)),
    );
    g.connect_new(idle, seq);
    g.connect_new(wave, seq);
    g.masks.push(Mask {
        joints: vec![MaskJoint {
            name: "Torso".into(),
            weight: 0.,
            children: Vec::new(),
        }],
        ..Mask::new("NoTorso")
    });
    let mask = g.add(NodeKind::Mask);
    g.nodes[mask]
        .props
        .insert("Mask".into(), Prop::Value(Value::Text("NoTorso".into())));
    g.connect(seq, mask, "Pose");
    g.connect(mask, g.output().unwrap(), "Pose");
    let mut p = Player::new(g, &resolve);
    assert!(p.step(0.5).is_empty(), "the torso is masked off");
    p.step(0.6);
    assert_eq!(p.active(seq), Some(1));
    assert!(p.step(0.1).contains_key("Arm"), "an unlisted joint passes");
}

#[test]
fn random_sequence_is_reproducible_with_a_seed() {
    let build = || {
        let mut g = Graph::default();
        let a = clip_node(&mut g, "idle", "OnceAndHold");
        let b = clip_node(&mut g, "run", "OnceAndHold");
        let r = g.add(NodeKind::RandomSequence);
        g.nodes[r]
            .props
            .insert("Seed".into(), Prop::Value(Value::Number(7.)));
        g.connect_new(a, r);
        g.connect_new(b, r);
        g.connect(r, g.output().unwrap(), "Pose");
        (Player::new(g, &resolve), r)
    };
    let picks = |(mut p, r): (Player, usize)| -> Vec<usize> {
        (0..6)
            .map(|_| {
                p.step(1.01);
                p.active(r).unwrap()
            })
            .collect()
    };
    assert_eq!(picks(build()), picks(build()));
}
