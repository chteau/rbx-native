//! What a player's character is made of: our own `Animate` and `Health`
//! scripts, and the noob, the character of a player we have no avatar for
//! (classic yellow, blue and green blocks, our own R6 rig, no Roblox
//! assets).

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

pub mod animate;

/// Our own `Health` script: while the humanoid is hurt and alive it heals
/// 1% of `MaxHealth` a second, the rate the Humanoid docs give for the
/// default regeneration.
pub const HEALTH_SOURCE: &str = include_str!("character/Health.luau");

/// `BrickColor`s of the noob: Bright yellow, Bright blue, Br. yellowish
/// green.
const YELLOW: u32 = 24;
const BLUE: u32 = 23;
const GREEN: u32 = 37;

const ROOT_ROT: [f32; 9] = [-1., 0., 0., 0., 0., 1., 0., 1., 0.];
const RIGHT_ROT: [f32; 9] = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
const LEFT_ROT: [f32; 9] = [0., 0., -1., 0., 1., 0., 1., 0., 0.];
const IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

fn cf(p: [f32; 3], rotation: [f32; 9]) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: p[0],
            y: p[1],
            z: p[2],
        },
        rotation,
    })
}

fn v3(v: [f32; 3]) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v[0],
        y: v[1],
        z: v[2],
    })
}

/// Builds the noob, feet at the origin, facing -Z, as a root of `dom` (or
/// under `parent`). R6, so the R6 default animations drive it.
pub fn noob(dom: &mut WeakDom, name: &str, parent: Option<Ref>) -> Ref {
    let model = dom.new_instance("Model", name, parent);
    let parts: [(&str, [f32; 3], [f32; 3], u32); 7] = [
        ("HumanoidRootPart", [2., 2., 1.], [0., 3., 0.], BLUE),
        ("Torso", [2., 2., 1.], [0., 3., 0.], BLUE),
        ("Head", [2., 1., 1.], [0., 4.5, 0.], YELLOW),
        ("Left Arm", [1., 2., 1.], [-1.5, 3., 0.], YELLOW),
        ("Right Arm", [1., 2., 1.], [1.5, 3., 0.], YELLOW),
        ("Left Leg", [1., 2., 1.], [-0.5, 1., 0.], GREEN),
        ("Right Leg", [1., 2., 1.], [0.5, 1., 0.], GREEN),
    ];
    let mut refs = std::collections::HashMap::new();
    for (part_name, size, at, color) in parts {
        let part = dom.new_instance("Part", part_name, Some(model));
        let props = [
            ("size", v3(size)),
            ("CFrame", cf(at, IDENTITY)),
            ("BrickColor", Variant::BrickColor(color)),
            ("Anchored", Variant::Bool(false)),
            ("CanCollide", Variant::Bool(true)),
        ];
        for (key, value) in props {
            let _ = dom.set_property(part, key, value);
        }
        if part_name == "HumanoidRootPart" {
            let _ = dom.set_property(part, "Transparency", Variant::Float32(1.0));
        }
        refs.insert(part_name, part);
    }
    let joints: [(&str, &str, &str, [f32; 3], [f32; 3], [f32; 9]); 6] = [
        (
            "RootJoint",
            "HumanoidRootPart",
            "Torso",
            [0., 0., 0.],
            [0., 0., 0.],
            ROOT_ROT,
        ),
        (
            "Neck",
            "Torso",
            "Head",
            [0., 1., 0.],
            [0., -0.5, 0.],
            ROOT_ROT,
        ),
        (
            "Right Shoulder",
            "Torso",
            "Right Arm",
            [1., 0.5, 0.],
            [-0.5, 0.5, 0.],
            RIGHT_ROT,
        ),
        (
            "Left Shoulder",
            "Torso",
            "Left Arm",
            [-1., 0.5, 0.],
            [0.5, 0.5, 0.],
            LEFT_ROT,
        ),
        (
            "Right Hip",
            "Torso",
            "Right Leg",
            [1., -1., 0.],
            [0.5, 1., 0.],
            RIGHT_ROT,
        ),
        (
            "Left Hip",
            "Torso",
            "Left Leg",
            [-1., -1., 0.],
            [-0.5, 1., 0.],
            LEFT_ROT,
        ),
    ];
    for (joint_name, part0, part1, c0, c1, rotation) in joints {
        let joint = dom.new_instance("Motor6D", joint_name, Some(refs[part0]));
        let props = [
            ("Part0", Variant::Ref(refs[part0])),
            ("Part1", Variant::Ref(refs[part1])),
            ("C0", cf(c0, rotation)),
            ("C1", cf(c1, rotation)),
        ];
        for (key, value) in props {
            let _ = dom.set_property(joint, key, value);
        }
    }
    let _ = dom.set_property(model, "PrimaryPart", Variant::Ref(refs["Head"]));
    let humanoid = dom.new_instance("Humanoid", "Humanoid", Some(model));
    let _ = dom.set_property(humanoid, "RigType", Variant::Enum(0));
    dom.new_instance("Animator", "Animator", Some(humanoid));
    let colors = dom.new_instance("BodyColors", "Body Colors", Some(model));
    for (key, color) in [
        ("HeadColor", YELLOW),
        ("LeftArmColor", YELLOW),
        ("RightArmColor", YELLOW),
        ("TorsoColor", BLUE),
        ("LeftLegColor", GREEN),
        ("RightLegColor", GREEN),
    ] {
        let _ = dom.set_property(colors, key, Variant::BrickColor(color));
    }
    finish(dom, model, false);
    model
}

/// Gives a character what every character has: a Humanoid's `Animator`,
/// our `Animate` (for its rig) and `Health` scripts, unless it brought its
/// own.
pub fn finish(dom: &mut WeakDom, model: Ref, r15: bool) {
    let child = |dom: &WeakDom, parent: Ref, name: &str| {
        dom.get(parent)?
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
    };
    if let Some(humanoid) = child(dom, model, "Humanoid") {
        let has_animator = dom.get(humanoid).is_some_and(|h| {
            h.children()
                .iter()
                .any(|c| dom.get(*c).is_some_and(|i| i.class() == "Animator"))
        });
        if !has_animator {
            dom.new_instance("Animator", "Animator", Some(humanoid));
        }
    }
    if child(dom, model, "Animate").is_none() {
        animate::add(dom, model, r15);
    }
    if child(dom, model, "Health").is_none() {
        let script = dom.new_instance("Script", "Health", Some(model));
        let _ = dom.set_property(script, "Source", Variant::String(HEALTH_SOURCE.into()));
    }
}
