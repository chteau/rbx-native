//! `build_rig`: a `Layout` made into instances.

use rbx_dom::{Color3Data, Content, Ref, Variant, Vector3Data, WeakDom};

use crate::explorer::insert::incremented_name;

use super::cframe::{vec3, Cf};
use super::layout::{Layout, PartSpec};
use super::proportions::{r15_sizes, RigType, Scales};
use super::{r15, r6, BodyColors, JointStyle, RigOptions};

const PLASTIC: u32 = 256;
const BLOCK: u32 = 1;
const FRONT: u32 = 5;
const HUMANOID_R6: u32 = 0;
const HUMANOID_R15: u32 = 1;
const HEAD_MESH: u32 = 0;
const FACE: &str = "rbxasset://textures/face.png";

/// The rig as a `Model` under `parent`, named from `options.name` made
/// unique among its siblings, with its feet on `options.feet`.
///
/// Only R15 carries the six `Body*Scale` values: an R6 `Humanoid` has none.
pub(crate) fn build_rig(dom: &mut WeakDom, options: &RigOptions, parent: Ref) -> Ref {
    let scales = options
        .scales
        .unwrap_or_else(|| Scales::preset(options.scale, options.shape));
    let r15_rig = options.rig_type == RigType::R15;
    let layout = if r15_rig {
        r15::layout(&r15_sizes(&scales), &scales, options.shape)
    } else {
        r6::layout()
    };
    let upgraded = r15_rig && options.joints == JointStyle::AnimationConstraint;
    let name = incremented_name(dom, Some(parent), &options.name);
    let model = dom.new_instance("Model", &name, Some(parent));

    let ground = layout.ground();
    let root = Cf::at([options.feet[0], options.feet[1] - ground, options.feet[2]]);
    let parts: Vec<(Ref, &PartSpec)> = layout
        .parts
        .iter()
        .map(|spec| (make_part(dom, model, spec, &root, &options.colors), spec))
        .collect();
    let part_ref = |name: &str| parts.iter().find(|(_, s)| s.name == name).map(|(r, _)| *r);

    for joint in &layout.joints {
        let (Some(part0), Some(part1), Some(host)) = (
            part_ref(&joint.part0),
            part_ref(&joint.part1),
            part_ref(&joint.host),
        ) else {
            continue;
        };
        if layout.rig_attachments {
            let a0 = rig_attachment(dom, part0, &joint.name, &joint.c0);
            let a1 = rig_attachment(dom, part1, &joint.name, &joint.c1);
            if upgraded {
                let constraint = dom.new_instance("AnimationConstraint", &joint.name, Some(host));
                set(dom, constraint, "Attachment0", Variant::Ref(a0));
                set(dom, constraint, "Attachment1", Variant::Ref(a1));
                continue;
            }
        }
        let motor = dom.new_instance("Motor6D", &joint.name, Some(host));
        set(dom, motor, "Part0", Variant::Ref(part0));
        set(dom, motor, "Part1", Variant::Ref(part1));
        set(dom, motor, "C0", Variant::CFrame(joint.c0.data()));
        set(dom, motor, "C1", Variant::CFrame(joint.c1.data()));
    }

    humanoid(dom, model, options.rig_type, &layout, &scales);
    if let Some(head) = part_ref("Head") {
        dress_head(dom, head);
    }
    if let Some(primary) = part_ref("HumanoidRootPart") {
        set(dom, model, "PrimaryPart", Variant::Ref(primary));
    }
    model
}

fn set(dom: &mut WeakDom, target: Ref, property: &str, value: Variant) {
    let _ = dom.set_property(target, property, value);
}

fn color3(rgb: [u8; 3]) -> Variant {
    let [r, g, b] = rgb.map(|c| f32::from(c) / 255.);
    Variant::Color3(Color3Data { r, g, b })
}

/// Which `BodyColors` slot paints a part.
pub(super) fn body_color(colors: &BodyColors, part: &str) -> [u8; 3] {
    let arm = part.ends_with("Arm") || part.ends_with("Hand");
    match (part, part.starts_with("Left"), part.starts_with("Right")) {
        ("Head", ..) => colors.head,
        (_, true, _) if arm => colors.left_arm,
        (_, true, _) => colors.left_leg,
        (_, _, true) if arm => colors.right_arm,
        (_, _, true) => colors.right_leg,
        _ => colors.torso,
    }
}

fn make_part(
    dom: &mut WeakDom,
    model: Ref,
    spec: &PartSpec,
    root: &Cf,
    colors: &BodyColors,
) -> Ref {
    let part = dom.new_instance("Part", &spec.name, Some(model));
    let root_part = spec.name == "HumanoidRootPart";
    let [r, g, b] = body_color(colors, &spec.name);
    let solid = root_part
        || matches!(
            spec.name.as_str(),
            "Head" | "Torso" | "UpperTorso" | "LowerTorso"
        );
    for (property, value) in [
        ("size", Variant::Vector3(vec3(spec.size))),
        ("CFrame", Variant::CFrame(root.mul(&spec.cf).data())),
        ("Color3uint8", Variant::Color3uint8 { r, g, b }),
        ("Material", Variant::Enum(PLASTIC)),
        ("shape", Variant::Enum(BLOCK)),
        ("Anchored", Variant::Bool(false)),
        ("CanCollide", Variant::Bool(solid)),
        (
            "Transparency",
            Variant::Float32(if root_part { 1. } else { 0. }),
        ),
    ] {
        set(dom, part, property, value);
    }
    for (attachment, cf) in &spec.attachments {
        let node = dom.new_instance("Attachment", attachment, Some(part));
        set(dom, node, "CFrame", Variant::CFrame(cf.data()));
    }
    part
}

fn rig_attachment(dom: &mut WeakDom, part: Ref, joint: &str, cf: &Cf) -> Ref {
    let node = dom.new_instance("Attachment", &format!("{joint}RigAttachment"), Some(part));
    set(dom, node, "CFrame", Variant::CFrame(cf.data()));
    node
}

fn humanoid(dom: &mut WeakDom, model: Ref, rig: RigType, layout: &Layout, scales: &Scales) {
    let humanoid = dom.new_instance("Humanoid", "Humanoid", Some(model));
    let (kind, hip) = match rig {
        RigType::R6 => (HUMANOID_R6, 0.),
        RigType::R15 => (HUMANOID_R15, layout.hip_height),
    };
    set(dom, humanoid, "RigType", Variant::Enum(kind));
    set(dom, humanoid, "HipHeight", Variant::Float32(hip));
    dom.new_instance("Animator", "Animator", Some(humanoid));
    if rig == RigType::R6 {
        return;
    }
    for (name, value) in [
        ("BodyDepthScale", scales.depth),
        ("BodyHeightScale", scales.height),
        ("BodyWidthScale", scales.width),
        ("BodyTypeScale", scales.body_type),
        ("BodyProportionScale", scales.proportion),
        ("HeadScale", scales.head),
    ] {
        let number = dom.new_instance("NumberValue", name, Some(humanoid));
        set(dom, number, "Value", Variant::Float64(f64::from(value)));
    }
}

/// The default face, and the rounded head shape Roblox's heads are drawn
/// with (the block under it is only the part's collision box).
fn dress_head(dom: &mut WeakDom, head: Ref) {
    let mesh = dom.new_instance("SpecialMesh", "Mesh", Some(head));
    set(dom, mesh, "MeshType", Variant::Enum(HEAD_MESH));
    set(
        dom,
        mesh,
        "Scale",
        Variant::Vector3(Vector3Data {
            x: 1.25,
            y: 1.25,
            z: 1.25,
        }),
    );
    let face = dom.new_instance("Decal", "face", Some(head));
    set(
        dom,
        face,
        "Texture",
        Variant::Content(Content::Uri(FACE.into())),
    );
    set(dom, face, "Face", Variant::Enum(FRONT));
}
