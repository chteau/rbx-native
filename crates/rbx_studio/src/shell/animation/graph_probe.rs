//! Builds `GraphProbe.rbxl`, a place for settling in real Studio what the
//! Animation Graph runtime here only guesses at: the RandomSequence seed and
//! Blend2D and Over mixing. It uses only node types the shipping Graph
//! Editor offers (no state machine: Studio's Insert Node menu has none). Run with
//!
//! ```text
//! GRAPH_PROBE_OUT=/path/GraphProbe.rbxl cargo test -p rbx_studio graph_probe -- --ignored
//! ```
//!
//! which also writes `STEPS.md` (graph_probe_steps.md) beside it. Playing an
//! unpublished graph: the script registers the `AnimationGraphDefinition`
//! with `AnimationClipProvider:RegisterAnimationClip`, as the Animation
//! Editor's own Animate template does; STEPS.md has the publish fallback.
//!
//! The place holds an R15 `ProbeRig` (Motor6D joints, its root anchored,
//! no `Animate` script), its clips as KeyframeSequences and its graphs in
//! `ServerStorage.RBX_ANIMSAVES.ProbeRig`, and `graph_probe.luau` as a
//! server Script that plays them and prints `PROBE` lines.

use std::f32::consts::PI;
use std::path::PathBuf;

use glam::Vec3;
use rbx_animation::graph::{self, Graph, NodeKind, Param, ParamKind, Prop, Value};
use rbx_animation::{Clip, Frame, Marker, PoseKey, Rig};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

use crate::anim_editor::place;
use crate::save::{save, Format};
use crate::shell::rig::{build_rig, BodyScale, BodyShape, JointStyle, RigOptions, RigType};

const JOINT: &str = "RightUpperArm";
const SOURCE: &str = include_str!("graph_probe.luau");
const STEPS: &str = include_str!("graph_probe_steps.md");

fn rad(degrees: f32) -> f32 {
    degrees * PI / 180.
}

/// A clip holding one pose, keyed at 0 and 1 s so it has a length.
fn held(name: &str, pose: Frame) -> Clip {
    let mut clip = Clip {
        name: name.into(),
        looped: true,
        ..Clip::default()
    };
    clip.set_pose(0., JOINT, PoseKey::new(pose));
    clip.set_pose(1., JOINT, PoseKey::new(pose));
    clip
}

/// A RandomSequence input: the arm at `degrees` about X, rising 0.5 studs
/// along Y over 0.6 s so that a repeated pick shows, with a `Pick` marker.
fn pick(index: usize, degrees: f32) -> Clip {
    let mut clip = Clip {
        name: format!("Pick{index}"),
        ..Clip::default()
    };
    let turn = Frame::angles(rad(degrees), 0., 0.);
    clip.set_pose(0., JOINT, PoseKey::new(turn));
    let risen = Frame::new(Vec3::new(0., 0.5, 0.), turn.rotation);
    clip.set_pose(0.6, JOINT, PoseKey::new(risen));
    clip.keyframe_mut(0.).markers.push(Marker {
        name: "Pick".into(),
        value: index.to_string(),
    });
    clip
}

fn clips() -> Vec<Clip> {
    let mut all = vec![
        held("ArmUp", Frame::angles(rad(90.), 0., 0.)),
        held("XPos90", Frame::angles(rad(90.), 0., 0.)),
        held("XNeg90", Frame::angles(rad(-90.), 0., 0.)),
        held("ZPos90", Frame::angles(0., 0., rad(90.))),
        held("ZNeg90", Frame::angles(0., 0., rad(-90.))),
        held("X120", Frame::angles(rad(120.), 0., 0.)),
        held("Z120", Frame::angles(0., 0., rad(120.))),
    ];
    for (i, degrees) in [0., 30., 60., 90.].into_iter().enumerate() {
        all.push(pick(i, degrees));
    }
    all
}

fn set(g: &mut Graph, node: usize, key: &str, value: Value) {
    g.nodes[node].props.insert(key.into(), Prop::Value(value));
}

fn clip_node(g: &mut Graph, clip: &Clip, at: (f32, f32)) -> usize {
    let n = g.add(NodeKind::Clip);
    g.nodes[n].assign_clip(&clip.name, clip.length(), clip.looped);
    if !clip.looped {
        set(g, n, "PlayMode", Value::Text("Once".into()));
    }
    g.nodes[n].position = at;
    n
}

fn new_graph(name: &str) -> Graph {
    let mut g = Graph {
        name: name.into(),
        ..Graph::default()
    };
    let out = g.output().unwrap();
    g.nodes[out].position = (700., 0.);
    g
}

fn number_param(g: &mut Graph, name: &str) {
    g.params.push(Param {
        name: name.into(),
        kind: ParamKind::Number,
        default: Value::Number(0.),
    });
}

fn graphs(clips: &[Clip]) -> Vec<Graph> {
    let by = |name: &str| clips.iter().find(|c| c.name == name).unwrap();
    let mut all = Vec::new();

    let mut g = new_graph("ProbeSanity");
    let c = clip_node(&mut g, by("ArmUp"), (300., 0.));
    let out = g.output().unwrap();
    g.connect(c, out, "Pose");
    all.push(g);

    for seed in [42., 7.] {
        let mut g = new_graph(&format!("ProbeRandom{seed}"));
        let rs = g.add(NodeKind::RandomSequence);
        g.nodes[rs].position = (350., 0.);
        for i in 0..4 {
            let c = clip_node(
                &mut g,
                by(&format!("Pick{i}")),
                (0., i as f32 * 140. - 210.),
            );
            g.connect(c, rs, &format!("Pick{i}"));
        }
        set(&mut g, rs, "Seed", Value::Number(seed));
        set(&mut g, rs, "DefaultTransitionDuration", Value::Number(0.));
        let out = g.output().unwrap();
        g.connect(rs, out, "Pose");
        all.push(g);
    }

    let mut g = new_graph("ProbeBlend2D");
    let blend = g.add(NodeKind::Blend2D);
    g.nodes[blend].position = (350., 0.);
    let inputs = [
        ("ZPos90", 1., 0.),
        ("ZNeg90", -1., 0.),
        ("XPos90", 0., 1.),
        ("XNeg90", 0., -1.),
    ];
    for (i, (name, x, y)) in inputs.into_iter().enumerate() {
        let c = clip_node(&mut g, by(name), (0., i as f32 * 140. - 210.));
        g.connect(c, blend, name);
        let input = g.nodes[blend]
            .inputs
            .iter_mut()
            .find(|p| p.pin == name)
            .unwrap();
        input
            .props
            .insert("X".into(), Prop::Value(Value::Number(x)));
        input
            .props
            .insert("Y".into(), Prop::Value(Value::Number(y)));
    }
    number_param(&mut g, "X");
    number_param(&mut g, "Y");
    g.nodes[blend]
        .props
        .insert("X".into(), Prop::Param("X".into()));
    g.nodes[blend]
        .props
        .insert("Y".into(), Prop::Param("Y".into()));
    let out = g.output().unwrap();
    g.connect(blend, out, "Pose");
    all.push(g);

    let mut g = new_graph("ProbeOver");
    let base = clip_node(&mut g, by("X120"), (0., -100.));
    let over = clip_node(&mut g, by("Z120"), (0., 100.));
    let node = g.add(NodeKind::Over);
    g.nodes[node].position = (350., 0.);
    g.connect(base, node, "Base");
    g.connect(over, node, "Over");
    number_param(&mut g, "W");
    g.nodes[node]
        .props
        .insert("Weight".into(), Prop::Param("W".into()));
    let out = g.output().unwrap();
    g.connect(node, out, "Pose");
    all.push(g);

    all
}

fn put(dom: &mut WeakDom, r: Ref, key: &str, value: Variant) {
    let _ = dom.set_property(r, key, value);
}

/// The whole place.
fn build() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    // Workspace.AuthorityMode Automatic: the editor's Animate template says
    // an unpublished graph previews only in that mode.
    put(&mut dom, workspace, "AuthorityMode", Variant::Enum(1));
    dom.new_instance("ServerStorage", "ServerStorage", None);
    let scripts = dom.new_instance("ServerScriptService", "ServerScriptService", None);

    let base = dom.new_instance("Part", "Baseplate", Some(workspace));
    put(&mut dom, base, "Anchored", Variant::Bool(true));
    put(&mut dom, base, "Locked", Variant::Bool(true));
    put(
        &mut dom,
        base,
        "size",
        Variant::Vector3(Vector3Data {
            x: 64.,
            y: 1.,
            z: 64.,
        }),
    );
    put(
        &mut dom,
        base,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.,
                y: -0.5,
                z: 0.,
            },
            rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        }),
    );

    let mut options = RigOptions::new(
        RigType::R15,
        BodyShape::Masculine,
        BodyScale::Classic,
        JointStyle::Motor6D,
    );
    options.name = "ProbeRig".into();
    let model = build_rig(&mut dom, &options, workspace);
    for &child in dom.get(model).unwrap().children().to_vec().iter() {
        let (class, name) = {
            let i = dom.get(child).unwrap();
            (i.class().to_owned(), i.name().to_owned())
        };
        if name == "Animate" {
            dom.remove(child);
        } else if class.ends_with("Part") && name == "HumanoidRootPart" {
            put(&mut dom, child, "Anchored", Variant::Bool(true));
        }
    }

    let rig = Rig::read(&dom, model);
    let folder = place::save_folder(&mut dom, model);
    let clips = clips();
    for clip in &clips {
        rbx_animation::sequence::write(&mut dom, Some(folder), clip, Some(&rig));
    }
    for g in graphs(&clips) {
        graph::dom::write(&mut dom, Some(folder), &g);
    }

    let script = dom.new_instance("Script", "GraphProbe", Some(scripts));
    put(&mut dom, script, "Source", Variant::String(SOURCE.into()));
    dom
}

#[test]
#[ignore = "writes GraphProbe.rbxl; set GRAPH_PROBE_OUT"]
fn graph_probe() {
    let path = std::env::var_os("GRAPH_PROBE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("GraphProbe.rbxl"));
    let dom = build();
    save(&dom, Format::Binary, &path).unwrap();
    let steps = path.with_file_name("STEPS.md");
    std::fs::write(&steps, STEPS).unwrap();
    eprintln!("wrote {} and {}", path.display(), steps.display());
}

#[test]
fn the_probe_place_holds_every_graph_its_script_plays() {
    let dom = build();
    let names: Vec<String> = place::all_of_class(&dom, "AnimationGraphDefinition", None)
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    for name in [
        "ProbeSanity",
        "ProbeRandom42",
        "ProbeRandom7",
        "ProbeBlend2D",
        "ProbeOver",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} missing");
        assert!(SOURCE.contains(name), "{name} not played");
    }
    for clip in clips() {
        assert!(place::sequence_named(&dom, &clip.name, None).is_some());
    }
}

#[test]
fn the_steps_name_every_graph_and_the_fallback() {
    for name in [
        "ProbeSanity",
        "ProbeRandom42",
        "ProbeRandom7",
        "ProbeBlend2D",
        "ProbeOver",
    ] {
        assert!(STEPS.contains(name), "{name} not in STEPS.md");
    }
    assert!(STEPS.contains("_AssetId") && STEPS.contains("PROBE done"));
    assert!(SOURCE.contains("_AssetId") && SOURCE.contains(r#"out("done")"#));
}

#[test]
fn the_place_saves_and_every_graph_has_a_valid_output() {
    let dom = build();
    let path = std::env::temp_dir().join(format!("graph_probe_unit_{}.rbxl", std::process::id()));
    save(&dom, Format::Binary, &path).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    let _ = std::fs::remove_file(&path);
    for g in graphs(&clips()) {
        assert!(g.output().is_some(), "{} has no output", g.name);
    }
}
