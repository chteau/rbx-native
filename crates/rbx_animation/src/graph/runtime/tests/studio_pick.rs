//! Priority Select once its Triggers turn off, a pick from the rest pose, a
//! Mask naming no mask and phase sync, held to the numbers real Studio
//! printed for the matching cases of `tests/studio_graph_probe/unknowns.rs`.

use super::semantics::{output, param, set, set_pin};
use super::studio_flow::{at, close, probe_clip, steps};
use super::*;

/// A Priority Select over `clips` (OnceAndHold), Trigger i on Bool `A`,
/// `B`; `rule` sets one input's Interruptible, its InterruptibleTrigger
/// on `C`.
fn priority(clips: &[&str], duration: f64, rule: Option<(usize, &str)>) -> Player {
    let mut g = Graph::default();
    for name in ["A", "B", "C"] {
        param(&mut g, name, ParamKind::Bool);
    }
    let ps = g.add(NodeKind::PrioritySelect);
    set(
        &mut g,
        ps,
        "DefaultTransitionDuration",
        Value::Number(duration),
    );
    for (k, id) in clips.iter().enumerate() {
        let c = clip_node(&mut g, id, "OnceAndHold");
        g.connect_new(c, ps);
        let trigger = Prop::Param(["A", "B"][k].into());
        set_pin(&mut g, ps, k, "Trigger", trigger);
    }
    if let Some((k, rule)) = rule {
        let rule = Prop::Value(Value::Text(rule.into()));
        set_pin(&mut g, ps, k, "TransitionOverrideInterruptible", rule);
        set_pin(
            &mut g,
            ps,
            k,
            "InterruptibleTrigger",
            Prop::Param("C".into()),
        );
    }
    output(&mut g, ps);
    Player::new(g, &probe_clip)
}

/// Sets `params`, steps `n` times and checks P0's (x, y) against Studio's.
fn expect(p: &mut Player, params: &[(&str, bool)], n: usize, studio: (f32, f32), what: &str) {
    for &(name, on) in params {
        p.set(name, Value::Bool(on));
    }
    let p0 = at(&steps(p, n), "P0");
    close(p0.x, studio.0, &format!("{what} x"));
    close(p0.y, studio.1, &format!("{what} y"));
}

#[test]
fn priority_select_fades_to_rest_when_every_trigger_turns_off() {
    let mut p = priority(&["T0", "T1"], 0.3, None);
    expect(&mut p, &[], 3, (0., 0.), "none on");
    // From rest it cuts in; with A off it fades out over the 0.3 s.
    expect(&mut p, &[("A", true)], 3, (1., 0.), "A");
    expect(&mut p, &[("A", false)], 1, (0.88889, 0.), "A off");
    expect(&mut p, &[], 7, (0.11111, 0.), "fading");
    expect(&mut p, &[], 1, (0., 0.), "at rest");
    expect(&mut p, &[], 23, (0., 0.), "stays at rest");
    expect(&mut p, &[("B", true)], 3, (0., 1.), "B");
    expect(&mut p, &[("B", false)], 1, (0., 0.88889), "B off");
    expect(&mut p, &[], 11, (0., 0.), "at rest again");
    // Both on plays A; A off moves down to B.
    expect(
        &mut p,
        &[("A", true), ("B", true)],
        12,
        (1., 0.),
        "A over B",
    );
    expect(
        &mut p,
        &[("A", false)],
        1,
        (0.88889, 0.11111),
        "A off, B on",
    );
    expect(&mut p, &[], 8, (0., 1.), "on B");
}

#[test]
fn interruptible_guards_only_a_move_up() {
    // R0 above, Finished: its Trigger going off leaves it at once.
    let mut p = priority(&["R0", "R1"], 0., Some((0, "Finished")));
    expect(&mut p, &[("A", true)], 3, (1., 0.2), "R0");
    expect(
        &mut p,
        &[("A", false), ("B", true)],
        1,
        (2., 0.06667),
        "down",
    );
    let mut p = priority(&["R0", "R1"], 0., Some((0, "Finished")));
    expect(&mut p, &[("A", true)], 3, (1., 0.2), "R0");
    expect(&mut p, &[("A", false)], 1, (0., 0.), "to rest");
    let mut p = priority(&["R0", "R1"], 0., Some((0, "Trigger")));
    expect(&mut p, &[("A", true)], 3, (1., 0.2), "R0");
    expect(&mut p, &[("A", false), ("B", true)], 3, (2., 0.2), "down");
    // R1 below, Finished: A waits for R1 to end, even with B turned off.
    let mut p = priority(&["R0", "R1"], 0., Some((1, "Finished")));
    expect(&mut p, &[("B", true)], 3, (2., 0.2), "R1");
    expect(&mut p, &[("A", true)], 12, (2., 1.), "held");
    expect(&mut p, &[], 1, (1., 0.06667), "up once finished");
    let mut p = priority(&["R0", "R1"], 0., Some((1, "Finished")));
    expect(&mut p, &[("B", true)], 3, (2., 0.2), "R1");
    expect(
        &mut p,
        &[("A", true), ("B", false)],
        4,
        (2., 0.46665),
        "held, B off",
    );
}

#[test]
fn a_select_cuts_in_from_the_rest_pose() {
    let mut g = Graph::default();
    param(&mut g, "S", ParamKind::Text);
    let select = g.add(NodeKind::Select);
    set(
        &mut g,
        select,
        "DefaultTransitionDuration",
        Value::Number(0.3),
    );
    let c = clip_node(&mut g, "T0", "Loop");
    let pin = g.connect_new(c, select).unwrap();
    g.nodes[select]
        .props
        .insert("Selection".into(), Prop::Param("S".into()));
    output(&mut g, select);
    let mut p = Player::new(g, &probe_clip);
    close(at(&steps(&mut p, 3), "P0").x, 0., "nothing selected");
    p.set("S", Value::Text(pin));
    close(at(&steps(&mut p, 1), "P0").x, 1., "first step");
}

#[test]
fn a_mask_naming_no_mask_passes_everything() {
    for (mask, invert) in [("", false), ("Nope", false), ("Nope", true)] {
        let mut g = Graph::default();
        let node = g.add(NodeKind::Mask);
        set(&mut g, node, "Mask", Value::Text(mask.into()));
        set(&mut g, node, "Invert", Value::Bool(invert));
        let c = clip_node(&mut g, "Both", "Loop");
        g.connect(c, node, NodeKind::Mask.default_pins()[0]);
        output(&mut g, node);
        let pose = steps(&mut Player::new(g, &probe_clip), 3);
        for joint in ["P0", "P1"] {
            close(
                at(&pose, joint).x,
                1.,
                &format!("{mask:?} {invert} {joint}"),
            );
        }
    }
}

/// Blend1D between L1 (Position 0) and L2 (Position 1), run through the
/// probe's script: `start` for 37 steps, 0 for 25, 0.5 for one, then 0.25
/// for 37. Each row is (sample, P0.x, P1.x) as Studio printed it.
fn phase(sync: &str, start: f64, rows: &[(usize, f32, f32)]) {
    let mut g = Graph::default();
    param(&mut g, "X", ParamKind::Number);
    let blend = g.add(NodeKind::Blend1D);
    set(&mut g, blend, "PhaseSync", Value::Text(sync.into()));
    for (k, id) in ["L1", "L2"].into_iter().enumerate() {
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
    g.nodes[blend]
        .props
        .insert("Position".into(), Prop::Param("X".into()));
    output(&mut g, blend);
    let mut p = Player::new(g, &probe_clip);
    let mut script = vec![(Some(start), 1)];
    script.extend([(None, 3); 12]);
    script.push((Some(0.), 1));
    script.extend([(None, 3); 8]);
    script.extend([(Some(0.5), 1), (Some(0.25), 1)]);
    script.extend([(None, 3); 12]);
    for (i, (x, n)) in script.into_iter().enumerate() {
        if let Some(x) = x {
            p.set("X", Value::Number(x));
        }
        let pose = steps(&mut p, n);
        for &(_, p0, p1) in rows.iter().filter(|r| r.0 == i) {
            close(at(&pose, "P0").x, p0, &format!("{sync} #{i} P0"));
            close(at(&pose, "P1").x, p1, &format!("{sync} #{i} P1"));
        }
    }
}

#[test]
fn synced_blends_run_on_the_weighted_mean_duration() {
    // 1:1 runs both at 1/1.5 cycles a second, 3:1 at 1/1.25; at 1:0 L2,
    // weightless, keeps L1's phase (sample 22 shows it in step).
    phase(
        "Synced",
        0.5,
        &[
            (0, 0.01111, 0.01111),
            (12, 0.41111, 0.41111),
            (13, 0.85556, 0.),
            (21, 0.65556, 0.),
            (22, 0.33889, 0.33889),
            (23, 0.52833, 0.17611),
            (27, 0.01833, 0.00611),
            (35, 0.49833, 0.16611),
        ],
    );
    phase(
        "Synced",
        0.,
        &[
            (0, 0.03333, 0.),
            (21, 0.06666, 0.),
            (22, 0.04444, 0.04444),
            (23, 0.08667, 0.02889),
            (35, 0.05666, 0.01889),
        ],
    );
}

#[test]
fn unsynced_inputs_keep_their_own_clock_while_weightless() {
    phase(
        "Unsynced",
        0.5,
        &[
            (0, 0.01667, 0.00833),
            (12, 0.11667, 0.30833),
            (13, 0.26667, 0.),
            (22, 0.05, 0.025),
            (23, 0.1, 0.01667),
            (35, 0.25, 0.16667),
        ],
    );
}
