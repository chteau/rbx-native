//! Graphs for how long a trigger lasts, events across a transition, "(Any)"
//! re-entry and ties, Priority Select once its Triggers turn off, a Mask
//! naming no mask, and PhaseSync. Besides `T0..`, `R0..` and `Both` they play
//! `MA`/`MB` (one second loops holding P0/P1 at x = 1 with markers `Ev` "A0".."A3" at
//! 0, 0.25, 0.5, 0.75 and "B0".."B3" at 0.1, 0.35, 0.6, 0.85) and `L1`/`L2`
//! (loops moving P0/P1 from x = 0 to 1 over one and two seconds).

use rbx_animation::graph::model::{ParamKind, Transition};
use rbx_animation::graph::{Graph, NodeKind, Prop, Value};

use super::cases::{clip, finish, graph, param, set, value, Case, Read};

type Sample = (Vec<(&'static str, Value)>, usize);
/// A transition: from ("(Any)" when `None`), to, expression, priority,
/// duration.
type Hop<'a> = (Option<usize>, usize, &'a str, u32, f32);

fn on(name: &'static str, v: bool) -> Vec<(&'static str, Value)> {
    vec![(name, Value::Bool(v))]
}

fn singles(n: usize) -> Vec<Sample> {
    (0..n).map(|_| (vec![], 1)).collect()
}

fn case(g: Graph, samples: Vec<Sample>) -> Case {
    Case {
        graph: g,
        read: Read::Blend(samples),
    }
}

/// A state machine over `clips` with `transitions`, entering the first
/// state.
fn machine(
    name: &str,
    params: &[(&str, ParamKind)],
    clips: &[(&str, &str)],
    transitions: &[Hop],
) -> Graph {
    let mut g = graph(name);
    for &(p, kind) in params {
        param(&mut g, p, kind, Value::Bool(false));
    }
    let sm = g.add(NodeKind::StateMachine);
    for (k, &(id, mode)) in clips.iter().enumerate() {
        let c = clip(&mut g, id);
        value(&mut g, c, "PlayMode", Value::Text(mode.into()));
        let pin = g.connect_new(c, sm).unwrap();
        // Studio plays nothing until EntryState names a state.
        if k == 0 {
            value(&mut g, sm, "EntryState", Value::Text(pin));
        }
    }
    for &(from, to, expr, priority, duration) in transitions {
        let mut t = Transition::new(from, to);
        t.expression = expr.into();
        t.priority = priority;
        t.duration = duration;
        g.nodes[sm].transitions.push(t);
    }
    finish(&mut g, sm);
    g
}

/// A cycle T0 → T1 → T2 → T0, each hop on `Go`, so the state tells how
/// many evaluations saw `Go` true.
fn triggers() -> Vec<Case> {
    let hops = [
        (Some(0), 1, "Go", 1, 0.),
        (Some(1), 2, "Go", 1, 0.),
        (Some(2), 0, "Go", 1, 0.),
    ];
    let loops = [("T0", "Loop"), ("T1", "Loop"), ("T2", "Loop")];
    let go = || on("Go", true);
    let with = |key| {
        let mut p = go();
        p.push((key, Value::Bool(true)));
        p
    };
    let mut trig = vec![(vec![], 3), (go(), 0), (vec![], 1), (vec![], 1)];
    trig.extend([(with("_wait"), 0), (vec![], 1), (vec![], 1)]);
    trig.extend([(with("_zero"), 0), (vec![], 1), (vec![], 1)]);
    trig.extend([(go(), 2), (go(), 1), (go(), 1), (vec![], 1)]);
    let held = vec![(vec![], 3), (go(), 1), (vec![], 1), (vec![], 1)];
    let mut held = held;
    held.extend([(on("Go", false), 1), (vec![], 1)]);
    vec![
        case(
            machine("TrigChain", &[("Go", ParamKind::Trigger)], &loops, &hops),
            trig,
        ),
        case(
            machine("BoolChain", &[("Go", ParamKind::Bool)], &loops, &hops),
            held,
        ),
    ]
}

/// A half-second Select switch from `from` to `to`, read every step, to see
/// where the incoming clip's clock stands once the blend ends.
fn handover(name: &str, from: &str, to: &str) -> Case {
    let mut g = graph(name);
    param(&mut g, "S", ParamKind::Text, Value::Text(String::new()));
    let node = g.add(NodeKind::Select);
    value(
        &mut g,
        node,
        "DefaultTransitionDuration",
        Value::Number(0.5),
    );
    let pins: Vec<String> = [from, to]
        .iter()
        .map(|id| {
            let c = clip(&mut g, id);
            g.connect_new(c, node).unwrap()
        })
        .collect();
    set(&mut g, node, "Selection", Prop::Param("S".into()));
    finish(&mut g, node);
    let to = |k: usize| vec![("S", Value::Text(pins[k].clone()))];
    let mut s = vec![(to(0), 10), (to(1), 1)];
    s.extend(singles(40));
    case(g, s)
}

/// A one-second Linear switch from MA to MB, by Select and by a state
/// machine, read every step; and a Blend1D between them at and off a tie.
fn events() -> Vec<Case> {
    let mut g = graph("EvSelect");
    param(&mut g, "S", ParamKind::Text, Value::Text(String::new()));
    let node = g.add(NodeKind::Select);
    value(&mut g, node, "DefaultTransitionDuration", Value::Number(1.));
    let pins: Vec<String> = ["MA", "MB"]
        .iter()
        .map(|id| {
            let c = clip(&mut g, id);
            g.connect_new(c, node).unwrap()
        })
        .collect();
    set(&mut g, node, "Selection", Prop::Param("S".into()));
    finish(&mut g, node);
    let to = |k: usize| vec![("S", Value::Text(pins[k].clone()))];
    let mut select = vec![(to(0), 30), (to(1), 1)];
    select.extend(singles(39));
    let mut sm = vec![(vec![], 30), (on("Go", true), 1)];
    sm.extend(singles(39));

    let mut b = graph("EvBlend");
    param(&mut b, "X", ParamKind::Number, Value::Number(0.5));
    let blend = b.add(NodeKind::Blend1D);
    for (k, id) in ["MA", "MB"].iter().enumerate() {
        let c = clip(&mut b, id);
        let pin = b.connect_new(c, blend).unwrap();
        let input = b.nodes[blend].inputs.iter_mut().find(|i| i.pin == pin);
        let pos = Prop::Value(Value::Number(k as f64));
        input.unwrap().props.insert("Position".into(), pos);
    }
    set(&mut b, blend, "Position", Prop::Param("X".into()));
    finish(&mut b, blend);
    let x = |v| vec![("X", Value::Number(v))];
    vec![
        handover("Handover", "T1", "L1"),
        handover("EvHandover", "MA", "MB"),
        case(g, select),
        case(
            machine(
                "EvMachine",
                &[("Go", ParamKind::Bool)],
                &[("MA", "Loop"), ("MB", "Loop")],
                &[(Some(0), 1, "Go", 1, 1.)],
            ),
            sm,
        ),
        case(b, vec![(x(0.5), 30), (x(0.45), 30), (x(0.55), 30)]),
    ]
}

/// "(Any)" → the playing state, with and without a blend, and ties: two
/// equal-priority transitions out of T0, in both orders, eight runs each,
/// and an "(Any)" one against a direct one.
fn machines() -> Vec<Case> {
    let mut out = Vec::new();
    for (name, duration) in [("AnyReenter", 0.), ("AnyReenterBlend", 0.5)] {
        let g = machine(
            name,
            &[("Go", ParamKind::Bool), ("Other", ParamKind::Bool)],
            &[("R0", "OnceAndHold"), ("T1", "Loop")],
            &[(None, 0, "Go", 1, duration), (None, 1, "Other", 1, 0.)],
        );
        let mut s = vec![(vec![], 6), (on("Go", true), 1), (on("Go", false), 1)];
        s.extend(singles(3));
        s.extend([(on("Go", true), 1), (vec![], 1), (vec![], 1)]);
        s.extend([(on("Go", false), 1), (on("Other", true), 3)]);
        let mut back = on("Other", false);
        back.push(("Go", Value::Bool(true)));
        s.push((back, 1));
        s.extend(singles(14));
        out.push(case(g, s));
    }
    let loops = [("T0", "Loop"), ("T1", "Loop"), ("T2", "Loop")];
    let tie = vec![(vec![], 3), (on("Go", true), 1), (vec![], 1)];
    for run in 0..8 {
        for (name, order) in [("TieBC", [1, 2]), ("TieCB", [2, 1])] {
            let ts: Vec<_> = order.iter().map(|&to| (Some(0), to, "Go", 1, 0.)).collect();
            let g = machine(
                &format!("{name}{run}"),
                &[("Go", ParamKind::Bool)],
                &loops,
                &ts,
            );
            out.push(case(g, tie.clone()));
        }
        let g = machine(
            &format!("TieAny{run}"),
            &[("Go", ParamKind::Bool)],
            &loops,
            &[(None, 2, "Go", 1, 0.), (Some(0), 1, "Go", 1, 0.)],
        );
        out.push(case(g, tie.clone()));
    }
    // Which way Priority runs, and whether it outranks "(Any)" losing ties.
    let ranked: [(&str, [Hop; 2]); 3] = [
        (
            "PrioUp",
            [(Some(0), 1, "Go", 1, 0.), (Some(0), 2, "Go", 2, 0.)],
        ),
        (
            "PrioDown",
            [(Some(0), 1, "Go", 2, 0.), (Some(0), 2, "Go", 1, 0.)],
        ),
        (
            "PrioAny",
            [(None, 2, "Go", 2, 0.), (Some(0), 1, "Go", 1, 0.)],
        ),
    ];
    for (name, ts) in ranked {
        let g = machine(name, &[("Go", ParamKind::Bool)], &loops, &ts);
        out.push(case(g, tie.clone()));
    }
    // No EntryState: whether anything plays, and whether an "(Any)"
    // transition still enters a state. `N` shows GetParameter's value
    // before any SetParameter.
    let mut g = machine(
        "NoEntry",
        &[("Go", ParamKind::Bool)],
        &loops,
        &[(None, 1, "Go", 1, 0.)],
    );
    param(&mut g, "N", ParamKind::Number, Value::Number(2.));
    for node in &mut g.nodes {
        node.props.remove("EntryState");
    }
    out.push(case(g, tie.clone()));
    out
}

/// A Priority Select over `clips`, Trigger i on parameter `A`, `B`, ...;
/// `rules` gives an input's Interruptible override and InterruptibleTrigger.
fn priority(name: &str, clips: &[&str], duration: f64, rules: &[(usize, &str)]) -> Graph {
    let mut g = graph(name);
    let names = ["A", "B", "C"];
    for p in names {
        param(&mut g, p, ParamKind::Bool, Value::Bool(false));
    }
    let node = g.add(NodeKind::PrioritySelect);
    value(
        &mut g,
        node,
        "DefaultTransitionDuration",
        Value::Number(duration),
    );
    for (k, id) in clips.iter().enumerate() {
        let c = clip(&mut g, id);
        value(&mut g, c, "PlayMode", Value::Text("OnceAndHold".into()));
        let pin = g.connect_new(c, node).unwrap();
        let input = g.nodes[node].inputs.iter_mut().find(|i| i.pin == pin);
        let props = &mut input.unwrap().props;
        props.insert("Trigger".into(), Prop::Param(names[k].into()));
        if let Some(&(_, rule)) = rules.iter().find(|r| r.0 == k) {
            let rule = Prop::Value(Value::Text(rule.into()));
            props.insert("TransitionOverrideInterruptible".into(), rule);
            props.insert("InterruptibleTrigger".into(), Prop::Param("C".into()));
        }
    }
    finish(&mut g, node);
    g
}

fn priorities() -> Vec<Case> {
    let both = |a, b| {
        let mut p = on("A", a);
        p.push(("B", Value::Bool(b)));
        p
    };
    let mut off = vec![(vec![], 3), (on("A", true), 3), (on("A", false), 1)];
    off.extend(singles(11));
    off.extend([(vec![], 20), (on("B", true), 3), (on("B", false), 1)]);
    off.extend(singles(11));
    off.extend([(both(true, true), 12), (on("A", false), 1)]);
    off.extend(singles(11));

    let mut fin_top = vec![(on("A", true), 3), (both(false, true), 1)];
    fin_top.extend(singles(19));
    let mut fin_bottom = vec![(on("B", true), 3), (on("A", true), 1)];
    fin_bottom.extend(singles(19));
    let mut fin_off = vec![(on("A", true), 3), (on("A", false), 1)];
    fin_off.extend(singles(19));
    let mut fin_bottom_off = vec![(on("B", true), 3), (both(true, false), 1)];
    fin_bottom_off.extend(singles(3));
    let mut trig_top = vec![(on("A", true), 3), (both(false, true), 3)];
    trig_top.extend([(on("C", true), 1), (vec![], 1)]);
    let cycles = ["T0", "T1"];
    let ramps = ["R0", "R1"];
    vec![
        case(priority("PSOff", &cycles, 0.3, &[]), off),
        case(
            priority("PSFinTop", &ramps, 0., &[(0, "Finished")]),
            fin_top,
        ),
        case(
            priority("PSFinBottom", &ramps, 0., &[(1, "Finished")]),
            fin_bottom,
        ),
        case(
            priority("PSFinOff", &ramps, 0., &[(0, "Finished")]),
            fin_off,
        ),
        case(
            priority("PSFinBottomOff", &ramps, 0., &[(1, "Finished")]),
            fin_bottom_off,
        ),
        case(
            priority("PSTrigTop", &ramps, 0., &[(0, "Trigger")]),
            trig_top,
        ),
    ]
}

/// A Mask over `Both` naming nothing, naming a mask that does not exist,
/// and the latter inverted.
fn masks() -> Vec<Case> {
    [
        ("MaskBlank", "", false),
        ("MaskNope", "Nope", false),
        ("MaskNopeInv", "Nope", true),
    ]
    .into_iter()
    .map(|(name, mask, invert)| {
        let mut g = graph(name);
        let node = g.add(NodeKind::Mask);
        value(&mut g, node, "Mask", Value::Text(mask.into()));
        value(&mut g, node, "Invert", Value::Bool(invert));
        let c = clip(&mut g, "Both");
        g.connect(c, node, NodeKind::Mask.default_pins()[0]);
        finish(&mut g, node);
        case(g, vec![(vec![], 3)])
    })
    .collect()
}

/// Blend1D between L1 (Position 0) and L2 (Position 1): even, then all on
/// L1 with L2 weightless, then 3:1.
fn phase(name: &str, sync: &str, start: f64) -> Case {
    let mut g = graph(name);
    param(&mut g, "X", ParamKind::Number, Value::Number(start));
    let blend = g.add(NodeKind::Blend1D);
    value(&mut g, blend, "PhaseSync", Value::Text(sync.into()));
    for (k, id) in ["L1", "L2"].iter().enumerate() {
        let c = clip(&mut g, id);
        let pin = g.connect_new(c, blend).unwrap();
        let input = g.nodes[blend].inputs.iter_mut().find(|i| i.pin == pin);
        let pos = Prop::Value(Value::Number(k as f64));
        input.unwrap().props.insert("Position".into(), pos);
    }
    set(&mut g, blend, "Position", Prop::Param("X".into()));
    finish(&mut g, blend);
    let x = |v| vec![("X", Value::Number(v))];
    let mut s = vec![(x(start), 1)];
    s.extend((0..12).map(|_| (vec![], 3)));
    s.push((x(0.), 1));
    s.extend((0..8).map(|_| (vec![], 3)));
    s.push((x(0.5), 1));
    s.push((x(0.25), 1));
    s.extend((0..12).map(|_| (vec![], 3)));
    case(g, s)
}

pub(crate) fn cases() -> Vec<Case> {
    let mut out = triggers();
    out.extend(events());
    out.extend(machines());
    out.extend(priorities());
    out.extend(masks());
    out.extend([
        phase("SyncHalf", "Synced", 0.5),
        phase("UnsyncHalf", "Unsynced", 0.5),
        phase("SyncFromL1", "Synced", 0.),
    ]);
    out
}
