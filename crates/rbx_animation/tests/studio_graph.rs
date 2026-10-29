//! Animation Graph maths against real Studio. `write_graphs_for_studio`
//! saves a place with a one-joint rig, constant-pose clips and one graph per
//! question (Blend2D weights, Over/Add/Subtract, a seeded RandomSequence),
//! the command-bar Lua that plays each graph on Studio's Animator and stores
//! the joint's `Transform` per sample in `Workspace.GraphDump`, and what our
//! `Player` gives for the same samples. `studio_matches_our_graphs` diffs
//! ours against `fixtures/studio_graph_dump.txt`, the `GraphDump.Value` of
//! the place Studio saved after running that Lua (Studio 0.745, 2026-10-28).

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;

use glam::Vec3;
use rbx_animation::curve::{file, CurveClip, FloatCurve, FloatKey, Interpolation};
use rbx_animation::graph::model::{Param, ParamKind};
use rbx_animation::graph::{self, Graph, NodeKind, Prop, Value};
use rbx_animation::{sequence, Clip, Frame, Priority, Rig};
use rbx_dom::{Ref, Variant, WeakDom};

const DT: f32 = 1. / 30.;

/// One graph and the samples taken of it: parameter values, then how many
/// steps of `DT` before the joint is read.
struct Case {
    graph: Graph,
    samples: Vec<(Vec<(&'static str, f64)>, usize)>,
}

fn clips() -> Vec<(&'static str, f32, Frame)> {
    vec![
        ("C0", 1., Frame::angles(0., FRAC_PI_2, 0.)),
        ("C1", 1., Frame::angles(FRAC_PI_2, 0., 0.)),
        ("C2", 1., Frame::at(Vec3::new(1., 0., 0.))),
        (
            "C3",
            1.,
            Frame::at(Vec3::new(0., 1., 0.)).mul(&Frame::angles(0., 0., FRAC_PI_2 / 2.)),
        ),
        ("R0", 0.5, Frame::at(Vec3::new(1., 0., 0.))),
        ("R1", 0.5, Frame::at(Vec3::new(0., 1., 0.))),
        ("R2", 0.5, Frame::at(Vec3::new(0., 0., 1.))),
    ]
}

fn number_param(g: &mut Graph, name: &str, default: f64) {
    g.params.push(Param {
        name: name.into(),
        kind: ParamKind::Number,
        default: Value::Number(default),
    });
}

fn clip_node(g: &mut Graph, id: &str) -> usize {
    let n = g.add(NodeKind::Clip);
    g.nodes[n]
        .props
        .insert("AnimationId".into(), Prop::Value(Value::Text(id.into())));
    n
}

fn into_output(g: &mut Graph, node: usize) {
    let out = g.output().unwrap();
    g.connect(node, out, "Pose");
}

fn blend2d() -> Case {
    let mut g = Graph {
        name: "Blend2D".into(),
        ..Graph::default()
    };
    number_param(&mut g, "X", 0.);
    number_param(&mut g, "Y", 0.);
    let blend = g.add(NodeKind::Blend2D);
    for (clip, x, y) in [
        ("C0", 0., 0.),
        ("C1", 1., 0.),
        ("C2", 0., 1.),
        ("C3", 1., 1.),
    ] {
        let c = clip_node(&mut g, clip);
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
    g.nodes[blend]
        .props
        .insert("X".into(), Prop::Param("X".into()));
    g.nodes[blend]
        .props
        .insert("Y".into(), Prop::Param("Y".into()));
    into_output(&mut g, blend);
    let at = |x, y| (vec![("X", x), ("Y", y)], 3);
    Case {
        graph: g,
        samples: vec![
            at(0., 0.),
            at(0.5, 0.5),
            at(0.25, 0.),
            at(0.8, 0.3),
            at(0.1, 0.9),
            at(0.5, 0.),
            at(2., 2.),
        ],
    }
}

fn layered(kind: NodeKind, name: &str, a: &str, b: &str) -> Case {
    let mut g = Graph {
        name: name.into(),
        ..Graph::default()
    };
    number_param(&mut g, "W", 1.);
    let node = g.add(kind);
    let pins: Vec<String> = kind.default_pins().iter().map(|p| p.to_string()).collect();
    let ca = clip_node(&mut g, a);
    let cb = clip_node(&mut g, b);
    g.connect(ca, node, &pins[0]);
    g.connect(cb, node, &pins[1]);
    g.nodes[node]
        .props
        .insert("Weight".into(), Prop::Param("W".into()));
    into_output(&mut g, node);
    let w = |w| (vec![("W", w)], 3);
    Case {
        graph: g,
        samples: vec![w(0.), w(0.3), w(0.7), w(1.), w(1.5)],
    }
}

fn random() -> Case {
    let mut g = Graph {
        name: "Random".into(),
        ..Graph::default()
    };
    let node = g.add(NodeKind::RandomSequence);
    g.nodes[node]
        .props
        .insert("Seed".into(), Prop::Value(Value::Number(7.)));
    g.nodes[node].props.insert(
        "DefaultTransitionDuration".into(),
        Prop::Value(Value::Number(0.)),
    );
    for clip in ["R0", "R1", "R2"] {
        let c = clip_node(&mut g, clip);
        g.nodes[c].props.insert(
            "PlayMode".into(),
            Prop::Value(Value::Text("OnceAndHold".into())),
        );
        g.connect_new(c, node).unwrap();
    }
    into_output(&mut g, node);
    // A sample in the middle of each of the first twelve plays.
    Case {
        graph: g,
        samples: (0..12)
            .map(|i| (vec![], if i == 0 { 7 } else { 15 }))
            .collect(),
    }
}

fn cases() -> Vec<Case> {
    vec![
        blend2d(),
        layered(NodeKind::Over, "Over", "C0", "C3"),
        layered(NodeKind::Add, "Add", "C0", "C3"),
        layered(NodeKind::Subtract, "Subtract", "C3", "C0"),
        random(),
    ]
}

fn clip_of(name: &str, length: f32, pose: Frame) -> Clip {
    let mut dom = WeakDom::new();
    let ks = sequence_instance(&mut dom, None, name, length, pose);
    sequence::read(&dom, ks)
}

/// A KeyframeSequence holding `pose` on joint `A` at 0 and at `length`.
fn sequence_instance(
    dom: &mut WeakDom,
    parent: Option<Ref>,
    name: &str,
    length: f32,
    pose: Frame,
) -> Ref {
    let ks = dom.new_instance("KeyframeSequence", name, parent);
    dom.set_property(ks, "Loop", Variant::Bool(true)).unwrap();
    dom.set_property(ks, "Priority", Variant::Enum(2)).unwrap();
    for time in [0., length] {
        let kf = dom.new_instance("Keyframe", "Keyframe", Some(ks));
        dom.set_property(kf, "Time", Variant::Float32(time))
            .unwrap();
        let root = dom.new_instance("Pose", "Root", Some(kf));
        dom.set_property(root, "CFrame", Variant::CFrame(Frame::IDENTITY.to_data()))
            .unwrap();
        dom.set_property(root, "Weight", Variant::Float32(0.))
            .unwrap();
        let a = dom.new_instance("Pose", "A", Some(root));
        dom.set_property(a, "CFrame", Variant::CFrame(pose.to_data()))
            .unwrap();
        dom.set_property(a, "Weight", Variant::Float32(1.)).unwrap();
    }
    ks
}

/// FACS channels (#184): a CurveAnimation with only face curves, one per
/// interpolation mode, sampled at these step counts of `DT`.
const FACE_STEPS: [usize; 5] = [3, 8, 15, 18, 27];

fn face_curves() -> BTreeMap<String, FloatCurve> {
    let curve = |mode, keys: &[(f32, f32)]| {
        FloatCurve::new(
            keys.iter()
                .map(|&(t, v)| FloatKey::new(t, v, mode))
                .collect(),
        )
    };
    let mut faces = BTreeMap::new();
    faces.insert(
        "JawDrop".into(),
        curve(Interpolation::Linear, &[(0., 0.), (1., 1.)]),
    );
    faces.insert(
        "LeftEyeClosed".into(),
        curve(Interpolation::Cubic, &[(0., 0.), (0.5, 1.), (1., 0.2)]),
    );
    faces.insert(
        "Corrugator".into(),
        curve(Interpolation::Constant, &[(0., 0.3), (0.5, 0.8), (1., 0.8)]),
    );
    faces
}

fn face_ours() -> Vec<String> {
    let faces = face_curves();
    let mut lines = Vec::new();
    for (i, steps) in FACE_STEPS.iter().enumerate() {
        let t = *steps as f32 * DT;
        let values: Vec<String> = faces
            .values()
            .map(|c| format!("{:.5}", c.eval(t).unwrap_or(0.)))
            .collect();
        lines.push(format!("Face,{i},{}", values.join(",")));
    }
    lines
}

fn face_lua() -> String {
    let names: Vec<String> = face_curves().keys().map(|k| format!("\"{k}\"")).collect();
    let steps: Vec<String> = FACE_STEPS.iter().map(|s| s.to_string()).collect();
    format!(
        r#"local frig = workspace.FaceRig
local fanimator = frig.AnimationController.Animator
local faces = frig.Head.FaceControls
for _, t in fanimator:GetPlayingAnimationTracks() do t:Stop(0) end
fanimator:StepAnimations(0)
local fanim = Instance.new("Animation")
fanim.AnimationId = KSP:RegisterKeyframeSequence(frig.FaceSaves.FaceAnim)
local ftrack = fanimator:LoadAnimation(fanim)
ftrack.Looped = false
ftrack:Play(0)
task.wait(1)
local done = 0
for i, n in {{{}}} do
  for _ = done + 1, n do fanimator:StepAnimations({DT}) end
  done = n
  local row = {{}}
  for _, name in {{{}}} do table.insert(row, string.format("%.5f", faces[name])) end
  table.insert(out, "Face," .. (i - 1) .. "," .. table.concat(row, ","))
end
"#,
        steps.join(","),
        names.join(","),
        DT = DT
    )
}

fn write_face_rig(dom: &mut WeakDom, workspace: Ref) {
    let model = dom.new_instance("Model", "FaceRig", Some(workspace));
    let part = |dom: &mut WeakDom, name: &str, y: f32| {
        let p = dom.new_instance("Part", name, Some(model));
        let at = Frame::at(Vec3::new(6., y, 0.)).to_data();
        dom.set_property(p, "CFrame", Variant::CFrame(at)).unwrap();
        dom.set_property(p, "Anchored", Variant::Bool(name == "HumanoidRootPart"))
            .unwrap();
        p
    };
    let root = part(dom, "HumanoidRootPart", 10.);
    let head = part(dom, "Head", 11.5);
    let neck = dom.new_instance("Motor6D", "Neck", Some(head));
    dom.set_property(neck, "Part0", Variant::Ref(root)).unwrap();
    dom.set_property(neck, "Part1", Variant::Ref(head)).unwrap();
    dom.new_instance("FaceControls", "FaceControls", Some(head));
    let controller = dom.new_instance("AnimationController", "AnimationController", Some(model));
    dom.new_instance("Animator", "Animator", Some(controller));
    let rig = Rig::read(dom, model);
    let saves = dom.new_instance("Folder", "FaceSaves", Some(model));
    let anim = file::CurveAnimation {
        curves: CurveClip {
            name: "FaceAnim".into(),
            looped: false,
            priority: Priority::Action,
            joints: BTreeMap::new(),
            markers: vec![],
        },
        faces: face_curves(),
        keyframe_names: vec![],
    };
    file::write(dom, Some(saves), &anim, Some(&rig));
}

fn out_dir() -> std::path::PathBuf {
    std::env::var("RBX_GRAPH_OUT")
        .expect("RBX_GRAPH_OUT")
        .into()
}

fn line(case: &str, sample: usize, f: &Frame) -> String {
    let d = f.to_data();
    let mut v = vec![d.position.x, d.position.y, d.position.z];
    v.extend(d.rotation);
    let nums: Vec<String> = v.iter().map(|n| format!("{n:.5}")).collect();
    format!("{case},{sample},{}", nums.join(","))
}

fn ours() -> Vec<String> {
    let clips: BTreeMap<String, Clip> = clips()
        .into_iter()
        .map(|(n, l, p)| (n.to_string(), clip_of(n, l, p)))
        .collect();
    let mut lines = Vec::new();
    for case in cases() {
        let name = case.graph.name.clone();
        let mut player = graph::Player::new(case.graph, &|id| clips.get(id).cloned());
        player.set_seed(7);
        for (i, (params, steps)) in case.samples.iter().enumerate() {
            for (p, v) in params {
                player.set(p, Value::Number(*v));
            }
            let mut pose = BTreeMap::new();
            for _ in 0..*steps {
                pose = player.step(DT);
            }
            let a = pose.get("A").copied().unwrap_or(Frame::IDENTITY);
            lines.push(line(&name, i, &a));
        }
    }
    lines
}

fn all_ours() -> Vec<String> {
    let mut lines = ours();
    lines.extend(face_ours());
    lines
}

/// Lua for Studio's command bar.
fn studio_lua() -> String {
    let mut cases_lua = String::new();
    for case in cases() {
        let samples: Vec<String> = case
            .samples
            .iter()
            .map(|(params, steps)| {
                let p: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!("{{p={{{}}},n={steps}}}", p.join(","))
            })
            .collect();
        cases_lua += &format!(
            "{{g=\"{}\",s={{{}}}}},\n",
            case.graph.name,
            samples.join(",")
        );
    }
    format!(
        r#"local KSP = game:GetService("KeyframeSequenceProvider")
local rig = workspace.GraphRig
local animator = rig.AnimationController.Animator
local joint = rig.A.J
local ids = {{}}
for _, ks in workspace.GraphClips:GetChildren() do ids[ks.Name] = KSP:RegisterKeyframeSequence(ks) end
for _, d in workspace.GraphDefs:GetDescendants() do
  if d:IsA("AnimationNodeDefinition") and ids[d:GetAttribute("AnimationId")] then d:SetAttribute("AnimationId", ids[d:GetAttribute("AnimationId")]) end
end
local cases = {{
{cases_lua}}}
local out = {{}}
for _, case in cases do
  for _, t in animator:GetPlayingAnimationTracks() do t:Stop(0) end
  animator:StepAnimations(0)
  local def = workspace.GraphDefs[case.g]
  local anim = Instance.new("Animation")
  anim.AnimationId = KSP:RegisterKeyframeSequence(def)
  local track = animator:LoadAnimation(anim)
  track:Play(0)
  -- The graph's clips load asynchronously; edit mode does not advance the
  -- Animator while this waits, so the samples still start at time 0.
  task.wait(1)
  for i, s in case.s do
    for k, v in s.p do track:SetParameter(k, v) end
    for _ = 1, s.n do animator:StepAnimations({DT}) end
    local c = {{joint.Transform:GetComponents()}}
    for j, x in c do c[j] = string.format("%.5f", x) end
    table.insert(out, case.g .. "," .. (i - 1) .. "," .. table.concat(c, ","))
  end
end
{face}local v = workspace:FindFirstChild("GraphDump") or Instance.new("StringValue")
v.Name = "GraphDump"
v.Value = table.concat(out, "\n")
v.Parent = workspace
print("GraphDump", #out)
"#,
        DT = DT,
        face = face_lua()
    )
}

#[test]
#[ignore = "writes a place for a real Studio comparison (RBX_GRAPH_OUT)"]
fn write_graphs_for_studio() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let base = std::fs::read(format!("{manifest}/../../assets/tests/TestPlace.rbxl")).unwrap();
    let mut dom = rbx_binary::deserialize(&base).unwrap();
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).unwrap().class() == "Workspace")
        .unwrap();

    let rig = dom.new_instance("Model", "GraphRig", Some(workspace));
    let part = |dom: &mut WeakDom, name: &str| {
        let p = dom.new_instance("Part", name, Some(rig));
        let at = Frame::at(Vec3::new(0., 10., 0.)).to_data();
        dom.set_property(p, "CFrame", Variant::CFrame(at)).unwrap();
        dom.set_property(p, "Anchored", Variant::Bool(name == "Root"))
            .unwrap();
        p
    };
    let root = part(&mut dom, "Root");
    let a = part(&mut dom, "A");
    dom.set_property(rig, "PrimaryPart", Variant::Ref(root))
        .unwrap();
    let j = dom.new_instance("Motor6D", "J", Some(a));
    dom.set_property(j, "Part0", Variant::Ref(root)).unwrap();
    dom.set_property(j, "Part1", Variant::Ref(a)).unwrap();
    let controller = dom.new_instance("AnimationController", "AnimationController", Some(rig));
    dom.new_instance("Animator", "Animator", Some(controller));

    let clip_folder = dom.new_instance("Folder", "GraphClips", Some(workspace));
    for (name, length, pose) in clips() {
        sequence_instance(&mut dom, Some(clip_folder), name, length, pose);
    }
    let defs = dom.new_instance("Folder", "GraphDefs", Some(workspace));
    for case in cases() {
        graph::dom::write(&mut dom, Some(defs), &case.graph);
    }
    write_face_rig(&mut dom, workspace);

    let dir = out_dir();
    std::fs::write(
        dir.join("graph180.rbxl"),
        rbx_binary::serialize(&dom).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("graph180.lua"), studio_lua()).unwrap();
    std::fs::write(dir.join("graph180_ours.txt"), all_ours().join("\n")).unwrap();
}

fn parse(text: &str) -> BTreeMap<(String, usize), Vec<f32>> {
    text.lines()
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let nums = f[2..].iter().map(|n| n.parse().unwrap()).collect();
            ((f[0].to_string(), f[1].parse().unwrap()), nums)
        })
        .collect()
}

#[test]
fn studio_matches_our_graphs() {
    let studio = parse(include_str!("fixtures/studio_graph_dump.txt"));
    let ours = parse(&all_ours().join("\n"));
    let mut worst: BTreeMap<String, f32> = BTreeMap::new();
    for (key, theirs) in &studio {
        let mine = &ours[key];
        let gap = theirs
            .iter()
            .zip(mine)
            .map(|(a, b)| (a - b).abs())
            .fold(0., f32::max);
        let entry = worst.entry(key.0.clone()).or_default();
        *entry = entry.max(gap);
        if gap > 1e-3 {
            eprintln!(
                "{} #{}: studio {theirs:?}\n      ours   {mine:?}",
                key.0, key.1
            );
        }
    }
    eprintln!("worst gap per graph: {worst:?}");
    assert!(worst.values().all(|&gap| gap <= 1e-3));
}
