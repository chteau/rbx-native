//! The Animation Editor end to end, through the real `rbxstudio` window: a
//! stock rig inserted, a walk keyed, saved as a `KeyframeSequence`, a
//! locomotion graph saved beside it, the place written with Save As, and the
//! editor quit. Every step is driven by `RBX_STUDIO_ANIMATION` (see
//! `shell::animation::debug`), through the same calls the clicks make. The
//! file left behind is then read back and checked, once for each joint class
//! a rig can be built with.
//!
//! It opens a window, so it needs a display and a GPU (Mesa's lavapipe is
//! enough):
//! `RBX_STUDIO_E2E_DISPLAY=:2 cargo test -p rbx_studio --test animation_e2e -- --ignored`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use rbx_animation::graph::NodeKind;
use rbx_animation::{sequence, JointKind, Rig};
use rbx_dom::{Ref, Variant, WeakDom};

const DISPLAY: &str = "RBX_STUDIO_E2E_DISPLAY";
const PLACE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
);

fn child(dom: &WeakDom, of: Ref, name: &str) -> Ref {
    dom.get(of)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn root(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).unwrap().class() == class)
        .unwrap_or_else(|| panic!("no {class}"))
}

/// Runs one scripted session and returns the place it wrote.
fn session(display: &str, dir: &Path, steps: &str) -> WeakDom {
    let place = dir.join("place.rbxl");
    let written = dir.join("written.rbxl");
    std::fs::copy(PLACE, &place).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rbxstudio"))
        .arg(&place)
        .env("DISPLAY", display)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env_remove("RBX_API_KEY")
        .env(
            "RBX_STUDIO_ANIMATION",
            format!("{steps},write={},quit", written.display()),
        )
        .spawn()
        .expect("rbxstudio is built alongside this test");
    let deadline = Instant::now() + Duration::from_secs(240);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("rbxstudio did not quit within four minutes");
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    assert!(status.success(), "rbxstudio exited with {status}");
    rbx_binary::deserialize(&std::fs::read(&written).expect("the place was written")).unwrap()
}

fn check(dom: &WeakDom, kind: JointKind) {
    let workspace = root(dom, "Workspace");
    let model = child(dom, workspace, "Rig");
    let rig = Rig::read(dom, model);
    assert_eq!(rig.joints.len(), 15, "an R15 rig has fifteen joints");
    assert!(
        rig.joints.iter().all(|j| j.kind == kind),
        "every joint is a {kind:?}"
    );

    // Saved at rest: every part sits where its joints put it with no
    // Transform, whatever pose the editor was showing.
    let root_frame = match dom
        .get(rig.root.unwrap())
        .unwrap()
        .properties()
        .get("CFrame")
    {
        Some(Variant::CFrame(c)) => rbx_animation::Frame::from_data(c),
        _ => panic!("the root part has a CFrame"),
    };
    for (part, expected) in rig.layout(root_frame, &|_| None) {
        let Some(Variant::CFrame(c)) = dom.get(part).unwrap().properties().get("CFrame") else {
            continue;
        };
        let actual = rbx_animation::Frame::from_data(c);
        assert!(
            actual.approx_eq(&expected, 1e-3),
            "{} was saved posed",
            rig.part_name(part)
        );
    }

    let saves = child(
        dom,
        child(dom, root(dom, "ServerStorage"), "RBX_ANIMSAVES"),
        "Rig",
    );
    let walk = sequence::read(dom, child(dom, saves, "Walk"));
    assert!(walk.looped);
    assert!((walk.length() - 1.2).abs() < 1e-4);
    assert!(walk.joints().len() >= 8, "{:?}", walk.joints());
    assert!(
        walk.joints().iter().all(|j| rig.joint_for(j).is_some()),
        "keys only real joints"
    );
    let markers: usize = walk.keyframes.iter().map(|k| k.markers.len()).sum();
    assert_eq!(markers, 2, "the two footstep events");

    let graph = rbx_animation::graph::dom::read(dom, child(dom, saves, "Locomotion"));
    let machine = graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::StateMachine)
        .expect("the locomotion state machine");
    assert_eq!(machine.transitions.len(), 2);
    let out = &graph.nodes[graph.output().unwrap()];
    assert!(out.inputs[0].node.is_some(), "the output is wired");

    let pointer = child(dom, model, "AnimSaves");
    assert_eq!(
        dom.get(pointer).unwrap().properties()["Value"],
        Variant::Ref(saves)
    );
}

#[test]
#[ignore = "opens a window: set RBX_STUDIO_E2E_DISPLAY and pass --ignored"]
fn keys_saves_and_writes_a_walk_for_either_joint_class() {
    let display = std::env::var(DISPLAY).expect("RBX_STUDIO_E2E_DISPLAY names an X display");
    for (build, kind) in [
        ("r15-motor6d", JointKind::Motor6D),
        ("r15-constraint", JointKind::AnimationConstraint),
    ] {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("rbx_anim_e2e_{build}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let steps = format!("build={build},demo,time=0.3,save,graph-demo,save-graph");
        let dom = session(&display, &dir, &steps);
        check(&dom, kind);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
