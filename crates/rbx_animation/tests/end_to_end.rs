//! A Blender export pasted in, saved as a `KeyframeSequence` beside an
//! Animation Graph, written to a real `.rbxm`, read back, and played on the
//! rig: the whole path a builder's animation takes, with no editor in it.

use std::f32::consts::FRAC_PI_2;
use std::io::Write;

use base64::Engine;
use glam::Vec3;
use rbx_animation::graph::{self, Graph, NodeKind, Prop, Value};
use rbx_animation::import::rbxanims;
use rbx_animation::{animator::Animator, sequence, Frame, Rig};
use rbx_dom::{Ref, Variant, WeakDom};

fn part(dom: &mut WeakDom, name: &str, parent: Ref, at: Vec3) -> Ref {
    let p = dom.new_instance("Part", name, Some(parent));
    dom.set_property(p, "CFrame", Variant::CFrame(Frame::at(at).to_data()))
        .unwrap();
    p
}

fn motor(dom: &mut WeakDom, name: &str, p0: Ref, p1: Ref, c0: Vec3, c1: Vec3) {
    let m = dom.new_instance("Motor6D", name, Some(p1));
    dom.set_property(m, "Part0", Variant::Ref(p0)).unwrap();
    dom.set_property(m, "Part1", Variant::Ref(p1)).unwrap();
    dom.set_property(m, "C0", Variant::CFrame(Frame::at(c0).to_data()))
        .unwrap();
    dom.set_property(m, "C1", Variant::CFrame(Frame::at(c1).to_data()))
        .unwrap();
}

/// The addon's clipboard text for an arm raised over one second.
fn pasted() -> String {
    let raised = Frame::angles(0., 0., FRAC_PI_2).to_data();
    let mut c = vec![0., 0., 0.];
    c.extend(raised.rotation);
    let json = serde_json::json!({
        "t": 1.0,
        "kfs": [{"t": 0, "kf": {}}, {"t": 1.0, "kf": {"RightUpperArm": c}}],
    });
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(json.to_string().as_bytes()).unwrap();
    base64::engine::general_purpose::STANDARD.encode(e.finish().unwrap())
}

#[test]
fn a_pasted_blender_animation_survives_a_file_and_plays() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Dummy", None);
    let root = part(&mut dom, "HumanoidRootPart", model, Vec3::new(0., 3., 0.));
    let torso = part(&mut dom, "UpperTorso", model, Vec3::new(0., 3., 0.));
    let arm = part(&mut dom, "RightUpperArm", model, Vec3::new(1.5, 3., 0.));
    motor(&mut dom, "Waist", root, torso, Vec3::ZERO, Vec3::ZERO);
    motor(
        &mut dom,
        "RightShoulder",
        torso,
        arm,
        Vec3::new(1., 0.5, 0.),
        Vec3::new(-0.5, 0.5, 0.),
    );

    let clip = rbxanims::decode(&pasted(), "Raise").unwrap();
    let rig = Rig::read(&dom, model);
    let saves = dom.new_instance("Folder", "AnimSaves", Some(model));
    sequence::write(&mut dom, Some(saves), &clip, Some(&rig));

    let mut g = Graph::default();
    let node = g.add(NodeKind::Clip);
    g.nodes[node].props.insert(
        "AnimationId".into(),
        Prop::Value(Value::Text("Raise".into())),
    );
    g.nodes[node].props.insert(
        "PlayMode".into(),
        Prop::Value(Value::Text("OnceAndHold".into())),
    );
    let out = g.output().unwrap();
    g.connect(node, out, "Pose");
    graph::dom::write(&mut dom, Some(saves), &g);

    let bytes = rbx_binary::serialize(&dom).unwrap();
    let loaded = rbx_binary::deserialize(&bytes).unwrap();
    let model = loaded.root_refs()[0];
    let saves = loaded
        .get(model)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| loaded.get(c).unwrap().name() == "AnimSaves")
        .unwrap();
    let find = |class: &str| {
        loaded
            .get(saves)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|&c| loaded.get(c).unwrap().class() == class)
            .unwrap()
    };

    let rig = Rig::read(&loaded, model);
    assert_eq!(rig.joints.len(), 2);
    let clip = sequence::read(&loaded, find("KeyframeSequence"));
    assert_eq!(clip.joints(), ["RightUpperArm"]);

    // Played through an Animator…
    let mut animator = Animator::new();
    let track = animator.load(clip.clone());
    animator.play(track, 0., 1., 1.);
    animator.step(1.0);
    let pose = animator.pose();
    let world = rig.layout(Frame::at(Vec3::new(0., 3., 0.)), &|j| pose.get(j).copied());
    let arm = rig.joint_for("RightUpperArm").unwrap().part1;
    assert!(
        world[&arm]
            .position
            .abs_diff_eq(Vec3::new(1.5, 4., 0.), 1e-3),
        "{:?}",
        world[&arm]
    );

    // …and through the graph read back from the file, which agree.
    let graph = graph::dom::read(&loaded, find("AnimationGraphDefinition"));
    let mut player = graph::Player::new(graph, &|id| (id == "Raise").then(|| clip.clone()));
    let from_graph = player.step(1.0);
    assert!(from_graph["RightUpperArm"].approx_eq(&pose["RightUpperArm"], 1e-4));
}
