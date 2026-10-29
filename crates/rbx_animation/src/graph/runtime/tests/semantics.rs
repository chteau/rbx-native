//! The documented node semantics, one behaviour per test.

use std::f64::consts::FRAC_PI_2;

use super::*;
use crate::graph::model::When;

pub(super) fn set(g: &mut Graph, node: usize, name: &str, value: Value) {
    g.nodes[node].props.insert(name.into(), Prop::Value(value));
}

pub(super) fn set_pin(g: &mut Graph, node: usize, pin: usize, name: &str, prop: Prop) {
    g.nodes[node].inputs[pin].props.insert(name.into(), prop);
}

pub(super) fn param(g: &mut Graph, name: &str, kind: ParamKind) {
    g.params.push(Param {
        name: name.into(),
        kind,
        default: kind.default_value(),
    });
}

pub(super) fn output(g: &mut Graph, from: usize) {
    let out = g.output().unwrap();
    g.connect(from, out, "Pose");
}

#[test]
fn trim_plays_only_the_trimmed_span() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "long", "Loop");
    set(&mut g, clip, "Trim", Value::Bool(true));
    set(&mut g, clip, "TrimStart", Value::Number(0.5));
    set(&mut g, clip, "TrimEnd", Value::Number(1.5));
    output(&mut g, clip);
    let mut p = Player::new(g, &resolve);
    assert!((x(&p.step(0.), "Leg") - 30.5).abs() < 1e-4);
    assert!((x(&p.step(0.75), "Leg") - 31.25).abs() < 1e-4);
    assert!(
        (x(&p.step(0.5), "Leg") - 30.75).abs() < 1e-4,
        "wrapped in the span"
    );
}

#[test]
fn once_and_reset_snaps_back_to_the_start() {
    let mut g = Graph::default();
    let clip = clip_node(&mut g, "idle", "OnceAndReset");
    output(&mut g, clip);
    let mut p = Player::new(g, &resolve);
    assert!((x(&p.step(0.5), "Torso") - 0.5).abs() < 1e-4);
    assert!(x(&p.step(0.6), "Torso").abs() < 1e-4);
    assert!(x(&p.step(0.3), "Torso").abs() < 1e-4, "and stays there");
}

/// A 1 s clip (Torso) and a 2 s one (Leg) blended half and half: the
/// virtual duration is 1.5 s, so 0.75 s is half a cycle for both.
#[test]
fn synced_blends_share_one_phase() {
    let build = |sync: &str| {
        let mut g = Graph::default();
        let short = clip_node(&mut g, "idle", "Loop");
        let long = clip_node(&mut g, "long", "Loop");
        let b = g.add(NodeKind::Blend1D);
        g.connect(short, b, "A");
        g.connect(long, b, "B");
        set(&mut g, b, "Position", Value::Number(0.5));
        set(&mut g, b, "PhaseSync", Value::Text(sync.into()));
        output(&mut g, b);
        Player::new(g, &resolve)
    };
    // Each joint comes through at half weight, so half its x.
    let mut synced = build("Synced");
    let pose = synced.step(0.75);
    assert!((x(&pose, "Torso") - 0.25).abs() < 1e-4, "{pose:?}");
    assert!((x(&pose, "Leg") - 15.5).abs() < 1e-4, "{pose:?}");
    let mut free = build("Unsynced");
    let pose = free.step(0.75);
    assert!((x(&pose, "Torso") - 0.375).abs() < 1e-4);
    assert!((x(&pose, "Leg") - 15.375).abs() < 1e-4);
}

fn priority(rule: &str) -> (Player, usize) {
    let mut g = Graph::default();
    for name in ["a", "b", "c"] {
        param(&mut g, name, ParamKind::Bool);
    }
    let idle = clip_node(&mut g, "idle", "Loop");
    let run = clip_node(&mut g, "run", "Loop");
    let ps = g.add(NodeKind::PrioritySelect);
    g.connect_new(idle, ps);
    g.connect_new(run, ps);
    set(&mut g, ps, "DefaultInterruptible", Value::Text(rule.into()));
    set(&mut g, ps, "DefaultTransitionDuration", Value::Number(0.));
    set_pin(&mut g, ps, 0, "Trigger", Prop::Param("a".into()));
    set_pin(&mut g, ps, 1, "Trigger", Prop::Param("b".into()));
    set_pin(
        &mut g,
        ps,
        1,
        "InterruptibleTrigger",
        Prop::Param("c".into()),
    );
    output(&mut g, ps);
    (Player::new(g, &resolve), ps)
}

#[test]
fn priority_select_takes_the_first_trigger_when_interruptible() {
    let (mut p, ps) = priority("Always");
    p.step(0.1);
    assert_eq!(p.active(ps), None, "nothing until one fires");
    p.set("b", Value::Bool(true));
    p.step(0.1);
    assert_eq!(p.active(ps), Some(1));
    p.set("a", Value::Bool(true));
    p.step(0.1);
    assert_eq!(p.active(ps), Some(0), "higher up wins at once");

    let (mut p, ps) = priority("Finished");
    p.set("b", Value::Bool(true));
    p.step(0.1);
    assert_eq!(p.active(ps), Some(1), "the first pick is never held back");
    p.set("a", Value::Bool(true));
    p.step(0.1);
    assert_eq!(p.active(ps), Some(1), "waits for the loop to end");
    p.step(0.85);
    p.step(0.);
    assert_eq!(p.active(ps), Some(0));

    let (mut p, ps) = priority("Trigger");
    p.set("b", Value::Bool(true));
    p.step(0.1);
    p.set("a", Value::Bool(true));
    p.step(1.5);
    assert_eq!(p.active(ps), Some(1));
    p.set("c", Value::Bool(true));
    p.step(0.);
    assert_eq!(p.active(ps), Some(0), "InterruptibleTrigger lets it go");
}

fn sequence(configure: impl Fn(&mut Graph, usize)) -> (Player, usize) {
    let mut g = Graph::default();
    param(&mut g, "go", ParamKind::Trigger);
    let idle = clip_node(&mut g, "idle", "OnceAndHold");
    let run = clip_node(&mut g, "run", "OnceAndHold");
    let seq = g.add(NodeKind::Sequence);
    g.connect_new(idle, seq);
    g.connect_new(run, seq);
    configure(&mut g, seq);
    output(&mut g, seq);
    (Player::new(g, &resolve), seq)
}

#[test]
fn before_finished_starts_the_blend_one_length_early() {
    let (mut p, seq) = sequence(|g, seq| {
        set(
            g,
            seq,
            "DefaultTransitionWhen",
            Value::Text("BeforeFinished".into()),
        );
        set(g, seq, "DefaultTransitionDuration", Value::Number(0.4));
    });
    p.step(0.5);
    assert_eq!(p.active(seq), Some(0));
    p.step(0.15);
    assert_eq!(
        p.active(seq),
        Some(1),
        "0.35 s left is within the 0.4 s blend"
    );

    let (mut p, seq) = sequence(|_, _| {});
    p.step(0.9);
    assert_eq!(p.active(seq), Some(0), "When Finished waits for the end");
    p.step(0.15);
    assert_eq!(p.active(seq), Some(1));
}

#[test]
fn a_sequence_can_wait_for_a_trigger_and_stops_after_its_loops() {
    let (mut p, seq) = sequence(|g, seq| {
        set(g, seq, "DefaultWaitFor", Value::Text("Trigger".into()));
        set_pin(g, seq, 0, "WaitForTrigger", Prop::Param("go".into()));
    });
    p.step(1.5);
    assert_eq!(p.active(seq), Some(0), "finishing is not enough");
    p.fire("go");
    p.step(0.);
    assert_eq!(p.active(seq), Some(1));

    let (mut p, seq) = sequence(|g, seq| {
        set(g, seq, "LoopCount", Value::Number(1.));
        set(g, seq, "DefaultTransitionDuration", Value::Number(0.));
    });
    p.step(1.01);
    assert_eq!(p.active(seq), Some(1));
    p.step(1.01);
    p.step(1.01);
    assert_eq!(
        p.active(seq),
        Some(1),
        "one loop, then the last input holds"
    );
}

fn one_shot(configure: impl Fn(&mut Graph, usize)) -> Player {
    let mut g = Graph::default();
    param(&mut g, "wave", ParamKind::Trigger);
    let idle = clip_node(&mut g, "idle", "Loop");
    let wave = clip_node(&mut g, "wave", "OnceAndHold");
    let shot = g.add(NodeKind::OneShot);
    g.nodes[shot]
        .props
        .insert("Trigger".into(), Prop::Param("wave".into()));
    g.connect(idle, shot, "Base");
    g.connect(wave, shot, "OneShot");
    configure(&mut g, shot);
    output(&mut g, shot);
    Player::new(g, &resolve)
}

#[test]
fn one_shot_blends_in_along_its_curve() {
    let mut p = one_shot(|g, shot| {
        set(g, shot, "TransitionInDuration", Value::Number(1.));
        set(g, shot, "TransitionInCurve", Value::Text("CubicV2".into()));
    });
    p.step(0.1);
    p.fire("wave");
    let arm = x(&p.step(0.25), "Arm");
    let w = curve_at(Style::CubicV2, 0.25);
    assert!((w - 0.25).abs() > 0.05, "eased, not linear");
    assert!((arm - w * 20.25).abs() < 1e-3, "{arm} vs {}", w * 20.25);
}

#[test]
fn an_uninterruptible_shot_ignores_new_triggers() {
    for (interruptible, expected) in [(true, 20.1), (false, 20.6)] {
        let mut p = one_shot(|g, shot| {
            set(g, shot, "TransitionInDuration", Value::Number(0.));
            set(g, shot, "Interruptible", Value::Bool(interruptible));
        });
        p.fire("wave");
        p.step(0.5);
        // One evaluation sees the trigger reset, so the next fire is an edge.
        p.step(0.);
        p.fire("wave");
        let arm = x(&p.step(0.1), "Arm");
        assert!((arm - expected).abs() < 1e-3, "{interruptible}: {arm}");
    }
}

#[test]
fn a_finished_transition_can_start_before_the_end() {
    let mut g = Graph::default();
    let idle = clip_node(&mut g, "idle", "OnceAndHold");
    let run = clip_node(&mut g, "run", "Loop");
    let sm = g.add(NodeKind::StateMachine);
    let entry = g.connect_new(idle, sm).unwrap();
    g.connect_new(run, sm);
    set(&mut g, sm, "EntryState", Value::Text(entry));
    let mut t = Transition::new(Some(0), 1);
    t.wait_for = WaitFor::Finished;
    t.when = When::BeforeFinished;
    t.duration = 0.3;
    g.nodes[sm].transitions.push(t);
    output(&mut g, sm);
    let mut p = Player::new(g, &resolve);
    p.step(0.6);
    p.step(0.);
    assert_eq!(p.active(sm), Some(0));
    p.step(0.15);
    p.step(0.);
    assert_eq!(p.active(sm), Some(1), "0.25 s left of a 0.3 s blend");
}

#[test]
fn blend2d_weighs_by_gradient_bands() {
    let build = |mode: &str, points: [(f64, f64); 2], at: (f64, f64)| {
        let mut g = Graph::default();
        let idle = clip_node(&mut g, "idle", "Loop");
        let run = clip_node(&mut g, "run", "Loop");
        let b = g.add(NodeKind::Blend2D);
        for (n, (px, py)) in [idle, run].into_iter().zip(points) {
            let pin = g.connect_new(n, b).unwrap();
            let k = g.nodes[b].inputs.iter().position(|i| i.pin == pin).unwrap();
            set_pin(&mut g, b, k, "X", Prop::Value(Value::Number(px)));
            set_pin(&mut g, b, k, "Y", Prop::Value(Value::Number(py)));
        }
        set(&mut g, b, "InputMode", Value::Text(mode.into()));
        set(&mut g, b, "X", Value::Number(at.0));
        set(&mut g, b, "Y", Value::Number(at.1));
        output(&mut g, b);
        x(&Player::new(g, &resolve).step(0.), "Torso")
    };
    let line = [(0., 0.), (1., 0.)];
    assert!(build("Cartesian", line, (0., 0.)).abs() < 1e-4);
    assert!((build("Cartesian", line, (1., 0.)) - 10.).abs() < 1e-4);
    assert!((build("Cartesian", line, (0.5, 0.)) - 5.).abs() < 1e-4);
    let compass = [(0., 1.), (FRAC_PI_2, 1.)];
    assert!(build("Polar", compass, (0., 1.)).abs() < 1e-4);
    let between = build("Polar", compass, (FRAC_PI_2 / 2., 1.));
    assert!((between - 5.).abs() < 1e-3, "{between}");
}

#[test]
fn an_interrupted_blend_restarts_from_its_target() {
    let mut g = Graph::default();
    param(&mut g, "pick", ParamKind::Text);
    let sel = g.add(NodeKind::Select);
    for id in ["idle", "run", "jog"] {
        let n = clip_node(&mut g, id, "Loop");
        g.connect(n, sel, id);
    }
    g.nodes[sel]
        .props
        .insert("Selection".into(), Prop::Param("pick".into()));
    set(&mut g, sel, "DefaultTransitionDuration", Value::Number(1.));
    output(&mut g, sel);
    let mut p = Player::new(g, &resolve);
    p.set("pick", Value::Text("idle".into()));
    p.step(0.);
    p.set("pick", Value::Text("run".into()));
    let mid = x(&p.step(0.5), "Torso");
    assert!((mid - 5.5).abs() < 1e-3, "{mid}");
    // Idle drops out at once; run (at 10.5 by now) fades into jog.
    p.set("pick", Value::Text("jog".into()));
    let after = x(&p.step(0.), "Torso");
    assert!((after - 10.5).abs() < 1e-3, "{after}");
}

#[test]
fn parameters_coerce_non_finite_numbers_to_zero() {
    let mut g = Graph::default();
    param(&mut g, "speed", ParamKind::Number);
    let mut p = Player::new(g, &resolve);
    p.set("speed", Value::Number(f64::NAN));
    assert_eq!(p.get("speed"), Some(&Value::Number(0.)));
}
