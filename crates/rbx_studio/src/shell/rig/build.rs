//! `build_rig`: a `Layout` made into instances.

use rbx_dom::{Content, Ref, Variant, Vector3Data, WeakDom};

use crate::explorer::insert::incremented_name;

use super::bundle::Piece;
use super::cframe::{vec3, Cf};
use super::layout::{Layout, PartSpec};
use super::proportions::{r15_sizes, RigType, Scales, Sizes};
use super::{animate, bundle, r15, r6, BodyColors, JointStyle, RigOptions};

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
        r15::layout(&sizes_for(options, &scales), options.shape, &options.pieces)
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
        dress_head(dom, head, r15_rig);
    }
    body_colors(dom, model, &options.colors);
    for (class, property) in [("Shirt", "ShirtTemplate"), ("Pants", "PantsTemplate")] {
        let clothing = dom.new_instance(class, class, Some(model));
        set(
            dom,
            clothing,
            property,
            Variant::Content(Content::Uri(String::new())),
        );
    }
    animate::add(dom, model, r15_rig);
    if let Some(primary) = part_ref("HumanoidRootPart") {
        set(dom, model, "PrimaryPart", Variant::Ref(primary));
    }
    model
}

/// The preset sizes, with each part of a replaced mesh stretched by how much
/// bigger its native size is than the stock mesh's.
fn sizes_for(options: &RigOptions, scales: &Scales) -> Sizes {
    let mut sizes = r15_sizes(scales);
    for (name, piece) in &options.pieces {
        let (Some(stock), Some(size)) = (bundle::piece(name), sizes.get_mut(name)) else {
            continue;
        };
        for i in 0..3 {
            size[i] *= piece.init[i] / stock.init[i];
        }
    }
    sizes
}

fn set(dom: &mut WeakDom, target: Ref, property: &str, value: Variant) {
    let _ = dom.set_property(target, property, value);
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
    let class = if spec.mesh.is_some() {
        "MeshPart"
    } else {
        "Part"
    };
    let part = dom.new_instance(class, &spec.name, Some(model));
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
        ("Anchored", Variant::Bool(false)),
        ("CanCollide", Variant::Bool(solid)),
        (
            "Transparency",
            Variant::Float32(if root_part { 1. } else { 0. }),
        ),
    ] {
        set(dom, part, property, value);
    }
    match &spec.mesh {
        Some(piece) => dress_mesh(dom, part, piece),
        None => set(dom, part, "shape", Variant::Enum(BLOCK)),
    }
    for (attachment, cf) in &spec.attachments {
        let node = dom.new_instance("Attachment", attachment, Some(part));
        set(dom, node, "CFrame", Variant::CFrame(cf.data()));
    }
    part
}

/// What makes a `MeshPart` the current-generation body part: its mesh, native
/// size, and the scale type and cage layered clothing wraps to.
fn dress_mesh(dom: &mut WeakDom, part: Ref, piece: &Piece) {
    set(dom, part, "MeshId", asset(piece.mesh));
    set(dom, part, "InitialSize", Variant::Vector3(vec3(piece.init)));
    if let Some(texture) = piece.texture {
        set(dom, part, "TextureID", asset(texture));
    }
    let original = dom.new_instance("Vector3Value", "OriginalSize", Some(part));
    set(dom, original, "Value", Variant::Vector3(vec3(piece.init)));
    let scale_type = dom.new_instance("StringValue", "AvatarPartScaleType", Some(part));
    set(dom, scale_type, "Value", Variant::String("Classic".into()));
    if let Some((cage, origin)) = piece.cage {
        let target = dom.new_instance("WrapTarget", "WrapTarget", Some(part));
        set(dom, target, "CageMeshId", asset(cage));
        set(
            dom,
            target,
            "CageOrigin",
            Variant::CFrame(Cf::at(origin).data()),
        );
    }
}

pub(super) fn asset(id: u64) -> Variant {
    Variant::Content(Content::Uri(format!("rbxassetid://{id}")))
}

fn body_colors(dom: &mut WeakDom, model: Ref, colors: &BodyColors) {
    let node = dom.new_instance("BodyColors", "Body Colors", Some(model));
    for (property, [r, g, b]) in [
        ("HeadColor3", colors.head),
        ("TorsoColor3", colors.torso),
        ("LeftArmColor3", colors.left_arm),
        ("RightArmColor3", colors.right_arm),
        ("LeftLegColor3", colors.left_leg),
        ("RightLegColor3", colors.right_leg),
    ] {
        set(dom, node, property, Variant::Color3uint8 { r, g, b });
    }
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

/// The default face. An R6 head also gets the rounded shape Roblox's old
/// heads are drawn with (the block under it is only the collision box); an
/// R15 head is already a mesh.
fn dress_head(dom: &mut WeakDom, head: Ref, r15: bool) {
    if !r15 {
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
    }
    let face = dom.new_instance("Decal", "face", Some(head));
    set(
        dom,
        face,
        "Texture",
        Variant::Content(Content::Uri(FACE.into())),
    );
    set(dom, face, "Face", Variant::Enum(FRONT));
}
