//! Markers climbing the graph as events, and the live weight per node.

use super::*;
use crate::clip::Marker;

/// `ramp(joint, x)` with a marker `name` at `at` seconds.
fn marked(joint: &str, x: f32, name: &str, at: f32) -> Clip {
    let mut clip = ramp(joint, x);
    clip.keyframe_mut(at).markers.push(Marker {
        name: name.into(),
        value: format!("{at}"),
    });
    clip
}

fn resolve_marked(id: &str) -> Option<Clip> {
    match id {
        "step" => Some(marked("Torso", 0., "Step", 0.5)),
        "start" => Some(marked("Torso", 0., "Start", 0.)),
        "wave" => Some(marked("Arm", 20., "Wave", 0.25)),
        other => resolve(other),
    }
}

fn names(p: &Player) -> Vec<(String, f32)> {
    p.events()
        .iter()
        .map(|e| (e.name.clone(), (e.weight * 100.).round() / 100.))
        .collect()
}

fn output(g: &mut Graph, from: usize) {
    let out = g.output().unwrap();
    g.connect(from, out, "Pose");
}

#[test]
fn a_looping_clip_raises_its_markers_each_pass() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "step", "Loop");
    output(&mut g, clip);
    let mut p = Player::new(g, &resolve_marked);
    p.step(0.4);
    assert!(p.events().is_empty());
    p.step(0.2);
    assert_eq!(names(&p), [("Step".into(), 1.)]);
    assert_eq!(p.events()[0].node, clip);
    p.step(0.2);
    assert!(p.events().is_empty());
    p.step(0.7);
    assert_eq!(names(&p), [("Step".into(), 1.)], "across the wrap");
}

#[test]
fn a_marker_at_the_start_fires_as_playback_begins() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "start", "OnceAndHold");
    output(&mut g, clip);
    let mut p = Player::new(g, &resolve_marked);
    p.step(0.1);
    assert_eq!(names(&p), [("Start".into(), 1.)]);
    p.step(0.1);
    assert!(p.events().is_empty());
}

#[test]
fn trimming_drops_the_markers_outside_the_span() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "step", "Loop");
    for (k, v) in [
        ("Trim", Value::Bool(true)),
        ("TrimStart", Value::Number(0.6)),
        ("TrimEnd", Value::Number(1.)),
    ] {
        g.nodes[clip].props.insert(k.into(), Prop::Value(v));
    }
    output(&mut g, clip);
    let mut p = Player::new(g, &resolve_marked);
    for _ in 0..10 {
        p.step(0.15);
        assert!(p.events().is_empty());
    }
}

#[test]
fn a_blend_passes_only_its_heaviest_input_scaled_by_its_weight() {
    let mut g = Graph::default();
    let a = clip_node(&mut g, "step", "Loop");
    let b = clip_node(&mut g, "wave", "Loop");
    let blend = g.add(NodeKind::Blend1D);
    g.connect(a, blend, "A");
    g.connect(b, blend, "B");
    g.nodes[blend]
        .props
        .insert("Position".into(), Prop::Value(Value::Number(0.25)));
    g.nodes[blend].props.insert(
        "PhaseSync".into(),
        Prop::Value(Value::Text("Unsynced".into())),
    );
    output(&mut g, blend);
    let mut p = Player::new(g, &resolve_marked);
    p.step(0.6);
    assert_eq!(names(&p), [("Step".into(), 0.75)], "Wave is silenced");
    assert!((p.weight(a) - 0.75).abs() < 1e-5);
    assert!((p.weight(b) - 0.25).abs() < 1e-5);
    assert_eq!(p.weight(blend), 1.);
}

#[test]
fn over_scales_its_top_layer_and_weight_zero_silences_it() {
    for (weight, expected) in [(0.5, vec![("Wave".to_owned(), 0.5)]), (0., vec![])] {
        let mut g = Graph::default();
        let base = clip_node(&mut g, "idle", "Loop");
        let top = clip_node(&mut g, "wave", "Loop");
        let over = g.add(NodeKind::Over);
        g.connect(base, over, "Base");
        g.connect(top, over, "Over");
        g.nodes[over]
            .props
            .insert("Weight".into(), Prop::Value(Value::Number(weight)));
        output(&mut g, over);
        let mut p = Player::new(g, &resolve_marked);
        p.step(0.3);
        assert_eq!(names(&p), expected);
    }
}

#[test]
fn a_transition_passes_both_sides_by_their_share() {
    let mut g = Graph::default();
    g.params.push(Param {
        name: "pick".into(),
        kind: ParamKind::Text,
        default: Value::Text("Clip".into()),
    });
    let idle = clip_node(&mut g, "idle", "Loop");
    let wave = clip_node(&mut g, "wave", "Loop");
    let select = g.add(NodeKind::Select);
    g.connect_new(idle, select);
    g.connect_new(wave, select);
    let props = &mut g.nodes[select].props;
    props.insert("Selection".into(), Prop::Param("pick".into()));
    props.insert(
        "DefaultTransitionDuration".into(),
        Prop::Value(Value::Number(1.)),
    );
    output(&mut g, select);
    let mut p = Player::new(g, &resolve_marked);
    p.step(0.1);
    assert_eq!(p.weight(wave), 0., "unselected inputs do not run");
    p.set("pick", Value::Text("Clip1".into()));
    // Wave restarts with the switch and passes its marker at 0.25 s, 0.3 of
    // the way into the blend.
    p.step(0.3);
    assert_eq!(names(&p), [("Wave".into(), 0.3)]);
    assert!((p.weight(idle) - 0.7).abs() < 1e-5);
}

#[test]
fn parameters_read_nil_until_set_and_keep_their_defaults() {
    let mut g = Graph::default();
    g.add_param(ParamKind::Number);
    let mut p = Player::new(g, &resolve);
    assert_eq!(p.get("NumberParameter"), None);
    assert_eq!(p.defaults()["NumberParameter"], Value::Number(0.));
    p.set("NumberParameter", Value::Number(3.));
    assert_eq!(p.get("NumberParameter"), Some(&Value::Number(3.)));
}
