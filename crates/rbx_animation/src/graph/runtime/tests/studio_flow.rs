//! Triggers, events across a transition and state-machine choices, held to
//! what real Studio was measured doing. Each test replays a case of
//! `tests/studio_graph_probe/unknowns.rs` on the same clips, stepped by
//! 1/30 s, against the numbers Studio printed
//! (`docs/research/animation-graph-measured.md`).

use super::semantics::{output, param, set, set_pin};
use super::*;
use crate::clip::Marker;

pub(super) const DT: f32 = 1. / 30.;

/// Keys at 0 and `length`; like the probe's, every clip poses both joints,
/// the ones it does not move at the origin.
fn keyed(joint: &str, from: Vec3, to: Vec3, length: f32) -> Clip {
    let mut clip = Clip::default();
    for (t, p) in [(0., from), (length, to)] {
        for j in ["P0", "P1"] {
            let p = if j == joint { p } else { Vec3::ZERO };
            clip.set_pose(t, j, PoseKey::new(Frame::at(p)));
        }
    }
    clip
}

fn hold(joint: &str, at: Vec3, length: f32) -> Clip {
    keyed(joint, at, at, length)
}

fn marked(joint: &str, prefix: &str, times: [f32; 4]) -> Clip {
    let mut clip = hold(joint, Vec3::X, 1.);
    for (k, t) in times.into_iter().enumerate() {
        clip.keyframe_mut(t).markers.push(Marker {
            name: "Ev".into(),
            value: format!("{prefix}{k}"),
        });
    }
    clip
}

/// The probe's clips: `T0..T2` hold P0 at 1 along x, y, z; `R0`/`R1` raise
/// P0's y from 0 to 1 over half a second at x = 1 / 2; `MA`/`MB` hold P0/P1
/// at x = 1 with markers; `L1`/`L2` move P0/P1 from x = 0 to 1 over one and
/// two seconds; `Both` holds P0 and P1 at x = 1.
pub(super) fn probe_clip(id: &str) -> Option<Clip> {
    let axis = [Vec3::X, Vec3::Y, Vec3::Z];
    Some(match id {
        "T0" | "T1" | "T2" => hold("P0", axis[usize::from(id.as_bytes()[1] - b'0')], 1.),
        "R0" | "R1" => {
            let x = f32::from(id.as_bytes()[1] - b'0') + 1.;
            keyed("P0", Vec3::new(x, 0., 0.), Vec3::new(x, 1., 0.), 0.5)
        }
        "MA" => marked("P0", "A", [0., 0.25, 0.5, 0.75]),
        "MB" => marked("P1", "B", [0.1, 0.35, 0.6, 0.85]),
        "L1" => keyed("P0", Vec3::ZERO, Vec3::X, 1.),
        "L2" => keyed("P1", Vec3::ZERO, Vec3::X, 2.),
        "Both" => {
            let mut clip = hold("P0", Vec3::X, 1.);
            for t in [0., 1.] {
                clip.set_pose(t, "P1", PoseKey::new(Frame::at(Vec3::X)));
            }
            clip
        }
        _ => return None,
    })
}

/// Steps `p` `n` times; the pose after the last step (empty for none).
pub(super) fn steps(p: &mut Player, n: usize) -> BTreeMap<String, Frame> {
    (0..n).fold(BTreeMap::new(), |_, _| p.step(DT))
}

pub(super) fn at(pose: &BTreeMap<String, Frame>, joint: &str) -> Vec3 {
    pose.get(joint).map_or(Vec3::ZERO, |f| f.position)
}

pub(super) fn close(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 2e-4, "{what}: ours {a}, Studio {b}");
}

/// A state machine over `clips` entering the first, with `transitions`
/// (from, to, expression, priority, duration) and Bool parameters.
fn machine(
    params: &[(&str, ParamKind)],
    clips: &[(&str, &str)],
    transitions: &[(Option<usize>, usize, &str, u32, f32)],
) -> (Player, usize) {
    let mut g = Graph::default();
    for &(name, kind) in params {
        param(&mut g, name, kind);
    }
    let sm = g.add(NodeKind::StateMachine);
    for &(id, mode) in clips {
        let c = clip_node(&mut g, id, mode);
        g.connect_new(c, sm);
    }
    let entry = g.nodes[sm].inputs[0].pin.clone();
    set(&mut g, sm, "EntryState", Value::Text(entry));
    for &(from, to, expression, priority, duration) in transitions {
        let mut t = Transition::new(from, to);
        t.expression = expression.into();
        t.priority = priority;
        t.duration = duration;
        g.nodes[sm].transitions.push(t);
    }
    output(&mut g, sm);
    (Player::new(g, &probe_clip), sm)
}

const LOOPS: [(&str, &str); 3] = [("T0", "Loop"), ("T1", "Loop"), ("T2", "Loop")];
const CYCLE: [(Option<usize>, usize, &str, u32, f32); 3] = [
    (Some(0), 1, "Go", 1, 0.),
    (Some(1), 2, "Go", 1, 0.),
    (Some(2), 0, "Go", 1, 0.),
];

#[test]
fn a_trigger_is_seen_by_exactly_one_step() {
    let (mut p, sm) = machine(&[("Go", ParamKind::Trigger)], &LOOPS, &CYCLE);
    // (fire, step 0 first, steps): Studio's state, then GetParameter("Go").
    // Waiting engine frames without a step changed nothing (sample 4).
    let script = [
        ((false, false, 3), 0, false),
        ((true, false, 0), 0, true),
        ((false, false, 1), 1, true),
        ((false, false, 1), 1, false),
        ((true, false, 0), 1, true),
        ((false, false, 1), 2, true),
        ((false, false, 1), 2, false),
        ((true, true, 0), 0, true),
        ((false, false, 1), 0, false),
        ((false, false, 1), 0, false),
        ((true, false, 2), 1, false),
        ((true, false, 1), 2, true),
        // Fired again while it still reads true: lost.
        ((true, false, 1), 2, false),
        ((false, false, 1), 2, false),
    ];
    for (i, ((fire, zero, n), state, reads)) in script.into_iter().enumerate() {
        if fire {
            p.set("Go", Value::Bool(true));
        }
        if zero {
            p.step(0.);
        }
        steps(&mut p, n);
        assert_eq!(p.active(sm), Some(state), "sample {i}");
        let go = p.get("Go").is_some_and(Value::truthy);
        assert_eq!(go, reads, "sample {i}: Go");
    }
}

#[test]
fn a_held_bool_takes_one_transition_per_step() {
    let (mut p, sm) = machine(&[("Go", ParamKind::Bool)], &LOOPS, &CYCLE);
    steps(&mut p, 3);
    p.set("Go", Value::Bool(true));
    let states: Vec<_> = (0..3)
        .map(|_| {
            steps(&mut p, 1);
            p.active(sm).unwrap()
        })
        .collect();
    assert_eq!(states, [1, 2, 0]);
}

/// Every event `p` reports over `n` steps, as `value@step`, counting on
/// from `from`.
fn events_over(p: &mut Player, n: usize, from: usize) -> Vec<String> {
    let mut out = Vec::new();
    for k in from + 1..=from + n {
        p.step(DT);
        out.extend(p.events().iter().map(|e| format!("{}@{k}", e.value)));
    }
    out
}

#[test]
fn both_sides_of_a_transition_fire_every_marker() {
    let (mut p, _) = machine(
        &[("Go", ParamKind::Bool)],
        &[("MA", "Loop"), ("MB", "Loop")],
        &[(Some(0), 1, "Go", 1, 1.)],
    );
    let mut seen = events_over(&mut p, 30, 0);
    p.set("Go", Value::Bool(true));
    // Up to the blend's end. Studio then fired MB's next markers a step
    // early (B0@63, B1@70), which is not reproduced here.
    seen.extend(events_over(&mut p, 30, 30));
    seen.sort_by_key(|e| e.split('@').nth(1).unwrap().parse::<usize>().unwrap());
    let studio = "A0@1 A1@8 A2@16 A3@23 A0@31 B0@34 A1@38 B1@41 A2@46 B2@49 \
                  A3@53 B3@56";
    assert_eq!(seen.join(" "), studio);
}

#[test]
fn a_blend_tie_passes_the_first_input_s_events() {
    let mut g = Graph::default();
    let blend = g.add(NodeKind::Blend1D);
    for (k, id) in ["MA", "MB"].into_iter().enumerate() {
        let c = clip_node(&mut g, id, "Loop");
        g.connect_new(c, blend);
        let pin = g.nodes[blend].inputs.len() - 1;
        set_pin(
            &mut g,
            blend,
            pin,
            "Position",
            Prop::Value(Value::Number(k as f64)),
        );
    }
    set(&mut g, blend, "Position", Value::Number(0.5));
    output(&mut g, blend);
    let mut p = Player::new(g, &probe_clip);
    assert_eq!(
        events_over(&mut p, 30, 0).join(" "),
        "A0@1 A1@8 A2@16 A3@23"
    );
}

#[test]
fn ties_go_to_the_state_s_own_then_the_first_listed() {
    let fork = |ts: &[(Option<usize>, usize, u32)]| {
        let ts: Vec<_> = ts.iter().map(|&(f, t, pr)| (f, t, "Go", pr, 0.)).collect();
        let (mut p, sm) = machine(&[("Go", ParamKind::Bool)], &LOOPS, &ts);
        steps(&mut p, 3);
        p.set("Go", Value::Bool(true));
        steps(&mut p, 1);
        p.active(sm).unwrap()
    };
    // Eight runs of each in Studio, all alike.
    assert_eq!(fork(&[(Some(0), 1, 1), (Some(0), 2, 1)]), 1, "TieBC");
    assert_eq!(fork(&[(Some(0), 2, 1), (Some(0), 1, 1)]), 2, "TieCB");
    assert_eq!(fork(&[(None, 2, 1), (Some(0), 1, 1)]), 1, "TieAny");
    assert_eq!(fork(&[(Some(0), 1, 1), (Some(0), 2, 2)]), 2, "PrioUp");
    assert_eq!(fork(&[(Some(0), 1, 2), (Some(0), 2, 1)]), 1, "PrioDown");
    assert_eq!(fork(&[(None, 2, 2), (Some(0), 1, 1)]), 2, "PrioAny");
}

#[test]
fn any_never_re_enters_the_playing_state() {
    for (duration, blended) in [(0., false), (0.5, true)] {
        let (mut p, _) = machine(
            &[("Go", ParamKind::Bool), ("Other", ParamKind::Bool)],
            &[("R0", "OnceAndHold"), ("T1", "Loop")],
            &[(None, 0, "Go", 1, duration), (None, 1, "Other", 1, 0.)],
        );
        // R0 at 0.2 s, then Go on, off, and held: R0 runs on regardless.
        let y = |pose: &BTreeMap<String, Frame>| at(pose, "P0").y;
        close(y(&steps(&mut p, 6)), 0.4, "R0 at 0.2 s");
        p.set("Go", Value::Bool(true));
        close(y(&steps(&mut p, 1)), 0.46667, "Go on");
        p.set("Go", Value::Bool(false));
        close(y(&steps(&mut p, 4)), 0.73333, "Go off");
        p.set("Go", Value::Bool(true));
        close(y(&steps(&mut p, 3)), 0.93333, "Go held");
        p.set("Go", Value::Bool(false));
        steps(&mut p, 1);
        p.set("Other", Value::Bool(true));
        close(at(&steps(&mut p, 3), "P0").x, 0., "on T1");
        // Back through "(Any)" from T1: R0 restarts, and holding Go keeps it.
        p.set("Other", Value::Bool(false));
        p.set("Go", Value::Bool(true));
        let pose = steps(&mut p, 1);
        let (x, y0) = if blended {
            (0.06667, 0.93778)
        } else {
            (1., 0.06667)
        };
        close(at(&pose, "P0").x, x, "re-entered x");
        close(y(&pose), y0, "re-entered y");
        let pose = steps(&mut p, 7);
        let y8 = if blended { 0.75111 } else { 0.53333 };
        close(y(&pose), y8, "held");
    }
}

#[test]
fn a_machine_without_an_entry_state_plays_nothing() {
    // Studio's NoEntry: the rest pose, before and after the "(Any)"
    // transition's condition turns on.
    let (p, _) = machine(
        &[("Go", ParamKind::Bool)],
        &LOOPS,
        &[(None, 1, "Go", 1, 0.)],
    );
    let mut g = p.graph().clone();
    for node in &mut g.nodes {
        node.props.remove("EntryState");
    }
    let mut p = Player::new(g, &probe_clip);
    assert_eq!(at(&steps(&mut p, 3), "P0"), Vec3::ZERO);
    // So the editor lights no state either.
    let sm = p
        .graph()
        .nodes
        .iter()
        .position(|n| n.kind == NodeKind::StateMachine);
    assert_eq!(p.active(sm.unwrap()), None);
    p.set("Go", Value::Bool(true));
    assert_eq!(at(&steps(&mut p, 1), "P0"), Vec3::ZERO);
    assert_eq!(at(&steps(&mut p, 1), "P0"), Vec3::ZERO);
}
