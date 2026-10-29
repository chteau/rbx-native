//! The graphs `write_probe_for_studio` plays, one per question, each over
//! the clips `T0..T11` (one joint moved along one axis), `R0..R4` (half-second
//! ramps told apart by x) and `Both`.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use rbx_animation::graph::model::{Mask, MaskJoint, Param, ParamKind};
use rbx_animation::graph::{Graph, NodeKind, Prop, Value};

const SEEDS: [f64; 8] = [0., 1., 2., 3., 7., 42., 1000., 123_456.];
pub(crate) const PICKS: usize = 40;

/// How a case is read: `Blend` prints every joint's position, the markers
/// reached and every parameter after each parameter set (a `_wait` key
/// waits one engine frame and `_zero` steps by 0 before stepping), `Picks`
/// prints the order RandomSequence played its inputs.
pub(crate) enum Read {
    Blend(Vec<(Vec<(&'static str, Value)>, usize)>),
    Picks,
}

pub(crate) struct Case {
    pub graph: Graph,
    pub read: Read,
}

pub(crate) fn set(g: &mut Graph, node: usize, key: &str, prop: Prop) {
    g.nodes[node].props.insert(key.into(), prop);
}

pub(crate) fn value(g: &mut Graph, node: usize, key: &str, v: Value) {
    set(g, node, key, Prop::Value(v));
}

pub(crate) fn param(g: &mut Graph, name: &str, kind: ParamKind, default: Value) {
    g.params.push(Param {
        name: name.into(),
        kind,
        default,
    });
}

pub(crate) fn graph(name: &str) -> Graph {
    Graph {
        name: name.into(),
        ..Graph::default()
    }
}

pub(crate) fn clip(g: &mut Graph, id: &str) -> usize {
    let n = g.add(NodeKind::Clip);
    value(g, n, "AnimationId", Value::Text(id.into()));
    n
}

pub(crate) fn finish(g: &mut Graph, node: usize) {
    let out = g.output().unwrap();
    g.connect(node, out, "Pose");
}

/// A Blend2D over `T0..` placed at `points`, sampled at `targets`.
fn blend2d(name: &str, polar: bool, points: &[(f64, f64)], targets: &[(f64, f64)]) -> Case {
    let mut g = graph(name);
    param(&mut g, "X", ParamKind::Number, Value::Number(0.));
    param(&mut g, "Y", ParamKind::Number, Value::Number(0.));
    let blend = g.add(NodeKind::Blend2D);
    if polar {
        value(&mut g, blend, "InputMode", Value::Text("Polar".into()));
    }
    for (k, &(x, y)) in points.iter().enumerate() {
        let c = clip(&mut g, &format!("T{k}"));
        let pin = g.connect_new(c, blend).unwrap();
        let input = g.nodes[blend]
            .inputs
            .iter_mut()
            .find(|i| i.pin == pin)
            .unwrap();
        input
            .props
            .insert("X".into(), Prop::Value(Value::Number(x)));
        input
            .props
            .insert("Y".into(), Prop::Value(Value::Number(y)));
    }
    set(&mut g, blend, "X", Prop::Param("X".into()));
    set(&mut g, blend, "Y", Prop::Param("Y".into()));
    finish(&mut g, blend);
    let samples = targets
        .iter()
        .map(|&(x, y)| (vec![("X", Value::Number(x)), ("Y", Value::Number(y))], 3))
        .collect();
    Case {
        graph: g,
        read: Read::Blend(samples),
    }
}

fn blends() -> Vec<Case> {
    let compass = |m: f64| [(0., m), (FRAC_PI_2, m), (PI, m), (-FRAC_PI_2, m)];
    let mut ring: Vec<(f64, f64)> = vec![(0., 0.)];
    ring.extend(compass(1.));
    let mut two_rings: Vec<(f64, f64)> = compass(1.).to_vec();
    two_rings.extend(compass(2.));
    let targets = [
        (0., 0.5),
        (FRAC_PI_4, 1.),
        (FRAC_PI_4, 0.5),
        (PI / 6., 1.),
        (3. * FRAC_PI_4, 0.7),
        (-FRAC_PI_4, 1.),
        (0., 2.),
        (FRAC_PI_2, 1.5),
        (0.3, 0.2),
        (5. * FRAC_PI_4, 1.),
        (2. * PI, 1.),
        (-PI, 1.),
        (FRAC_PI_4, 0.),
        (FRAC_PI_4, 1.5),
    ];
    vec![
        blend2d("PolarRing", true, &ring, &targets),
        blend2d("PolarTwoRings", true, &two_rings, &targets),
        blend2d(
            "PolarFan",
            true,
            &[(0., 1.), (1., 1.), (2.5, 1.)],
            &[
                (0.5, 1.),
                (0.25, 1.),
                (1.75, 1.),
                (0.5, 0.5),
                (0.5, 2.),
                (-0.5, 1.),
                (3., 1.),
                (4.5, 1.),
            ],
        ),
        blend2d(
            "PolarAsym",
            true,
            &[(0., 0.), (0., 1.), (1.2, 2.), (2.5, 0.7), (-1., 1.5)],
            &[
                (0.6, 1.5),
                (0.6, 1.),
                (0.3, 1.8),
                (1.8, 1.2),
                (-0.5, 1.2),
                (-0.5, 0.6),
                (3.5, 1.),
                (1., 0.4),
            ],
        ),
        blend2d(
            "Cartesian5",
            false,
            &[(0., 0.), (1., 0.), (0.3, 1.), (-1., 0.5), (0.5, -0.8)],
            &[
                (0.2, 0.2),
                (0.6, 0.4),
                (-0.5, 0.2),
                (0.1, -0.5),
                (2., 2.),
                (-0.2, 0.8),
            ],
        ),
    ]
}

/// A RandomSequence over `R0..R{n-1}` (OnceAndHold ramps), weights as given.
fn random(name: &str, node_id: &str, seed: f64, weights: &[f64]) -> Case {
    let mut g = graph(name);
    let node = g.add(NodeKind::RandomSequence);
    g.nodes[node].id = node_id.into();
    g.nodes[node].name = node_id.into();
    value(&mut g, node, "Seed", Value::Number(seed));
    value(&mut g, node, "DefaultTransitionDuration", Value::Number(0.));
    for (k, &w) in weights.iter().enumerate() {
        let c = clip(&mut g, &format!("R{k}"));
        value(&mut g, c, "PlayMode", Value::Text("OnceAndHold".into()));
        let pin = g.connect_new(c, node).unwrap();
        let input = g.nodes[node]
            .inputs
            .iter_mut()
            .find(|i| i.pin == pin)
            .unwrap();
        input
            .props
            .insert("Weight".into(), Prop::Value(Value::Number(w)));
    }
    finish(&mut g, node);
    Case {
        graph: g,
        read: Read::Picks,
    }
}

fn randoms() -> Vec<Case> {
    let mut out = Vec::new();
    for n in [2, 3, 5] {
        for seed in SEEDS {
            out.push(random(
                &format!("Rnd{n}s{seed}"),
                "Random",
                seed,
                &vec![1.; n],
            ));
        }
    }
    out.push(random("Rnd3s7w", "Random", 7., &[1., 2., 3.]));
    out.push(random("Rnd3s7named", "Other", 7., &[1., 1., 1.]));
    out.push(random("Rnd3sNeg", "Random", -1., &[1., 1., 1.]));
    out.push(random("Rnd3sNeg2", "Random", -2., &[1., 1., 1.]));
    out.push(random("Rnd3s7.5", "Random", 7.5, &[1., 1., 1.]));
    out
}

/// A Select over T0, T1, T2 switched by a Text parameter: one second
/// transitions with `curve`, read every step, then an interrupted one, then
/// a Selection naming no input.
fn select(name: &str, curve: &str) -> Case {
    let mut g = graph(name);
    param(&mut g, "S", ParamKind::Text, Value::Text(String::new()));
    let node = g.add(NodeKind::Select);
    value(&mut g, node, "DefaultTransitionDuration", Value::Number(1.));
    value(
        &mut g,
        node,
        "DefaultTransitionCurve",
        Value::Text(curve.into()),
    );
    let pins: Vec<String> = (0..3)
        .map(|k| {
            let c = clip(&mut g, &format!("T{k}"));
            g.connect_new(c, node).unwrap()
        })
        .collect();
    set(&mut g, node, "Selection", Prop::Param("S".into()));
    finish(&mut g, node);
    let to = |k: usize| vec![("S", Value::Text(pins[k].clone()))];
    let mut samples = vec![(to(0), 45)];
    samples.extend((0..32).map(|i| (if i == 0 { to(1) } else { vec![] }, 1)));
    samples.push((vec![], 15));
    samples.push((to(2), 15));
    samples.extend((0..32).map(|i| (if i == 0 { to(0) } else { vec![] }, 1)));
    samples.push((vec![("S", Value::Text("Nope".into()))], 3));
    samples.push((vec![], 40));
    Case {
        graph: g,
        read: Read::Blend(samples),
    }
}

fn misc() -> Vec<Case> {
    let mut out = vec![
        select("SelectCubic", "CubicV2"),
        select("SelectLinear", "Linear"),
    ];

    let mut g = graph("PriorityNone");
    let node = g.add(NodeKind::PrioritySelect);
    for k in 0..2 {
        let c = clip(&mut g, &format!("T{k}"));
        g.connect_new(c, node);
    }
    finish(&mut g, node);
    out.push(Case {
        graph: g,
        read: Read::Blend(vec![(vec![], 3), (vec![], 30)]),
    });

    // Joint P0 at half weight, P1 not listed; the clip moves both.
    let mut g = graph("MaskMissing");
    g.masks.push(Mask {
        joints: vec![MaskJoint {
            name: "P0".into(),
            weight: 0.5,
            children: Vec::new(),
        }],
        ..Mask::new("Half")
    });
    let node = g.add(NodeKind::Mask);
    value(&mut g, node, "Mask", Value::Text("Half".into()));
    let c = clip(&mut g, "Both");
    g.connect(c, node, NodeKind::Mask.default_pins()[0]);
    finish(&mut g, node);
    out.push(Case {
        graph: g,
        read: Read::Blend(vec![(vec![], 3)]),
    });

    // The half-second R0 ramp trimmed with TrimEnd before TrimStart.
    for (name, start, end) in [("TrimBackwards", 0.2, 0.), ("TrimBackwards2", 0.3, 0.1)] {
        let mut g = graph(name);
        let c = clip(&mut g, "R0");
        value(&mut g, c, "Trim", Value::Bool(true));
        value(&mut g, c, "TrimStart", Value::Number(start));
        value(&mut g, c, "TrimEnd", Value::Number(end));
        value(&mut g, c, "PlayMode", Value::Text("Loop".into()));
        finish(&mut g, c);
        let every = (0..12).map(|_| (vec![], 2)).collect();
        out.push(Case {
            graph: g,
            read: Read::Blend(every),
        });
    }

    // A Select whose Selection names nothing from the start, then a pin.
    let mut g = graph("SelectFromNone");
    param(&mut g, "S", ParamKind::Text, Value::Text(String::new()));
    let node = g.add(NodeKind::Select);
    value(
        &mut g,
        node,
        "DefaultTransitionDuration",
        Value::Number(0.3),
    );
    let c = clip(&mut g, "T0");
    let pin = g.connect_new(c, node).unwrap();
    set(&mut g, node, "Selection", Prop::Param("S".into()));
    finish(&mut g, node);
    let mut s = vec![(vec![], 3), (vec![("S", Value::Text(pin))], 1)];
    s.extend((0..4).map(|_| (vec![], 1)));
    out.push(Case {
        graph: g,
        read: Read::Blend(s),
    });

    // A Select whose Selection names nothing from the start.
    let mut g = graph("SelectEmpty");
    let node = g.add(NodeKind::Select);
    value(&mut g, node, "DefaultTransitionDuration", Value::Number(1.));
    for k in 0..2 {
        let c = clip(&mut g, &format!("T{k}"));
        g.connect_new(c, node);
    }
    finish(&mut g, node);
    out.push(Case {
        graph: g,
        read: Read::Blend(vec![(vec![], 1), (vec![], 14), (vec![], 30)]),
    });

    // A OneShot of the R0 ramp over T0, re-triggered while it plays.
    let mut g = graph("OneShotAgain");
    param(&mut g, "Go", ParamKind::Bool, Value::Bool(false));
    let node = g.add(NodeKind::OneShot);
    value(&mut g, node, "TransitionInDuration", Value::Number(0.));
    value(&mut g, node, "TransitionOutDuration", Value::Number(0.));
    set(&mut g, node, "Trigger", Prop::Param("Go".into()));
    let base = clip(&mut g, "T3");
    let shot = clip(&mut g, "R0");
    value(&mut g, shot, "PlayMode", Value::Text("OnceAndHold".into()));
    g.connect(base, node, "Base");
    g.connect(shot, node, "OneShot");
    finish(&mut g, node);
    let go = |on| vec![("Go", Value::Bool(on))];
    out.push(Case {
        graph: g,
        read: Read::Blend(vec![
            (vec![], 3),
            (go(true), 6),
            (go(false), 1),
            (go(true), 1),
            (vec![], 1),
            (vec![], 20),
        ]),
    });
    out
}

pub(crate) fn cases() -> Vec<Case> {
    let mut out = blends();
    out.extend(misc());
    out.extend(randoms());
    out.extend(super::unknowns::cases());
    out
}
