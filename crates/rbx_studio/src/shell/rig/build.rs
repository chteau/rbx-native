//! `build_rig`: a `Layout` made into instances, in the order, with the
//! children and the constraints of Roblox's own avatar models.

use std::collections::BTreeMap;

use rbx_dom::{Color3Data, Content, Ref, Variant, Vector3Data, WeakDom};

use crate::explorer::insert::incremented_name;

use super::bundle::{Family, Piece};
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
const ROOT_GREY: [u8; 3] = [163, 162, 165];

/// What `build_rig` makes, in the order the reference models list them.
enum Slot {
    Part(&'static str),
    Humanoid,
    Animate,
    Colors,
}

const R15_ORDER: [Slot; 19] = [
    Slot::Part("HumanoidRootPart"),
    Slot::Humanoid,
    Slot::Part("Head"),
    Slot::Animate,
    Slot::Part("UpperTorso"),
    Slot::Part("LowerTorso"),
    Slot::Part("RightUpperArm"),
    Slot::Part("RightLowerArm"),
    Slot::Part("RightHand"),
    Slot::Part("LeftUpperArm"),
    Slot::Part("LeftLowerArm"),
    Slot::Part("LeftHand"),
    Slot::Part("LeftUpperLeg"),
    Slot::Part("LeftLowerLeg"),
    Slot::Part("LeftFoot"),
    Slot::Part("RightUpperLeg"),
    Slot::Part("RightLowerLeg"),
    Slot::Part("RightFoot"),
    Slot::Colors,
];

const R6_ORDER: [Slot; 10] = [
    Slot::Part("Head"),
    Slot::Part("Torso"),
    Slot::Part("Left Arm"),
    Slot::Part("Right Arm"),
    Slot::Part("Left Leg"),
    Slot::Part("Right Leg"),
    Slot::Humanoid,
    Slot::Part("HumanoidRootPart"),
    Slot::Animate,
    Slot::Colors,
];

/// Which parts each R15 part is stopped from colliding with.
const NO_COLLISION: [(&str, &[&str]); 8] = [
    (
        "Head",
        &["HumanoidRootPart", "LeftUpperArm", "RightUpperArm"],
    ),
    (
        "UpperTorso",
        &[
            "HumanoidRootPart",
            "RightUpperLeg",
            "LeftUpperLeg",
            "LeftLowerLeg",
            "RightLowerLeg",
            "LeftLowerArm",
            "RightLowerArm",
        ],
    ),
    (
        "LowerTorso",
        &[
            "LeftUpperArm",
            "RightUpperArm",
            "LeftLowerLeg",
            "RightLowerLeg",
        ],
    ),
    ("RightUpperArm", &["RightHand"]),
    ("LeftUpperArm", &["LeftHand"]),
    ("LeftUpperLeg", &["RightUpperLeg", "LeftFoot"]),
    ("RightUpperLeg", &["RightFoot"]),
    ("LowerTorso", &[]),
];

/// `BallSocketConstraint` limits per joint, without the side: the cone and
/// the twist range, in degrees. `Root` is not limited.
const SOCKET_LIMITS: [(&str, f32, f32, f32); 8] = [
    ("Neck", 45., -40., 40.),
    ("Waist", 20., -40., 20.),
    ("Shoulder", 110., -85., 85.),
    ("Elbow", 20., 5., 120.),
    ("Wrist", 30., -10., 10.),
    ("Hip", 40., -5., 80.),
    ("Knee", 5., -120., -5.),
    ("Ankle", 10., -10., 10.),
];

/// Body part slots of a `HumanoidDescription`, one `BodyPartDescription` each.
const BODY_PARTS: u32 = 6;

/// The rig as a `Model` under `parent`, named from `options.name` made
/// unique among its siblings, with its feet on `options.feet`.
///
/// Only R15 carries the six `Body*Scale` values: an R6 `Humanoid` has none.
pub(crate) fn build_rig(dom: &mut WeakDom, options: &RigOptions, parent: Ref) -> Ref {
    let scales = options.scales();
    let r15_rig = options.rig_type == RigType::R15;
    let layout = if r15_rig {
        r15::layout(
            &sizes_for(options, &scales),
            options.hip_spread(),
            &options.pieces,
        )
    } else {
        r6::layout()
    };
    let upgraded = r15_rig && options.joints == JointStyle::AnimationConstraint;
    let name = incremented_name(dom, Some(parent), &options.name);
    let model = dom.new_instance("Model", &name, Some(parent));

    let ground = layout.ground();
    let root = Cf::at([options.feet[0], options.feet[1] - ground, options.feet[2]]);
    let mut parts: BTreeMap<&str, Ref> = BTreeMap::new();
    let mut rig_nodes: BTreeMap<(String, String), Ref> = BTreeMap::new();
    let order: &[Slot] = if r15_rig { &R15_ORDER } else { &R6_ORDER };
    for slot in order {
        match slot {
            Slot::Humanoid => humanoid(dom, model, options.rig_type, &layout, &scales),
            Slot::Animate => animate::add(dom, model, r15_rig),
            Slot::Colors => body_colors(dom, model, &options.colors),
            Slot::Part(name) => {
                let Some(spec) = layout.parts.iter().find(|spec| spec.name == *name) else {
                    continue;
                };
                let part = make_part(dom, model, spec, &layout, &root, &options.colors);
                rig_attachments(dom, part, spec, &layout, &mut rig_nodes);
                parts.insert(&spec.name, part);
            }
        }
    }

    if upgraded {
        no_collisions(dom, &parts);
    }
    for joint in &layout.joints {
        let (Some(&part0), Some(&part1), Some(&host)) = (
            parts.get(joint.part0.as_str()),
            parts.get(joint.part1.as_str()),
            parts.get(joint.host.as_str()),
        ) else {
            continue;
        };
        let nodes = (
            rig_nodes.get(&(joint.part0.clone(), joint.name.clone())),
            rig_nodes.get(&(joint.part1.clone(), joint.name.clone())),
        );
        if let (true, (Some(&a0), Some(&a1))) = (upgraded, nodes) {
            constraints(dom, host, &joint.name, a0, a1);
            continue;
        }
        let motor = dom.new_instance("Motor6D", &joint.name, Some(host));
        set(dom, motor, "Part0", Variant::Ref(part0));
        set(dom, motor, "Part1", Variant::Ref(part1));
        set(dom, motor, "C0", Variant::CFrame(joint.c0.data()));
        set(dom, motor, "C1", Variant::CFrame(joint.c1.data()));
    }

    if let Some(&primary) = parts.get("HumanoidRootPart") {
        set(dom, model, "PrimaryPart", Variant::Ref(primary));
    }
    model
}

/// The preset sizes, with each part of a replaced mesh stretched by how much
/// bigger its native size is than the stock mesh's.
fn sizes_for(options: &RigOptions, scales: &Scales) -> Sizes {
    let mut sizes = r15_sizes(scales, options.shape);
    for (name, piece) in &options.pieces {
        let (Some(stock), Some(size)) = (
            bundle::piece(options.family(), options.shape, name),
            sizes.get_mut(name),
        ) else {
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
        ("HumanoidRootPart", ..) => ROOT_GREY,
        (_, true, _) if arm => colors.left_arm,
        (_, true, _) => colors.left_leg,
        (_, _, true) if arm => colors.right_arm,
        (_, _, true) => colors.right_leg,
        _ => colors.torso,
    }
}

/// `SurfaceType`s of an R6 block part, top to right (`Smooth` elsewhere).
fn r6_surfaces(part: &str) -> [(&'static str, u32); 4] {
    const STUDS: u32 = 3;
    const INLET: u32 = 4;
    const WELD: u32 = 2;
    let (top, bottom, side) = match part {
        "Head" => (0, INLET, 0),
        "Torso" => (STUDS, INLET, WELD),
        "Left Arm" | "Right Arm" => (STUDS, INLET, 0),
        "Left Leg" | "Right Leg" => (STUDS, 0, 0),
        _ => (0, 0, 0),
    };
    [
        ("TopSurface", top),
        ("BottomSurface", bottom),
        ("LeftSurface", side),
        ("RightSurface", side),
    ]
}

fn make_part(
    dom: &mut WeakDom,
    model: Ref,
    spec: &PartSpec,
    layout: &Layout,
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
    let solid = match spec.name.as_str() {
        "Torso" | "UpperTorso" | "LowerTorso" => true,
        "HumanoidRootPart" => layout.rig_attachments,
        "Head" => spec
            .mesh
            .as_ref()
            .map_or(true, |piece| piece.family == Family::Classic),
        _ => false,
    };
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
    if spec.mesh.is_none() {
        set(dom, part, "shape", Variant::Enum(BLOCK));
        for (property, surface) in r6_surfaces(&spec.name) {
            if surface != 0 {
                set(dom, part, property, Variant::Enum(surface));
            }
        }
        if !layout.rig_attachments {
            dress_block(dom, part, &spec.name);
        }
    }
    part
}

/// The R6 face and the chest decal, and the rounded shape Roblox's old heads
/// are drawn with (the block under it is only the collision box).
fn dress_block(dom: &mut WeakDom, part: Ref, name: &str) {
    let decal = match name {
        "Head" => {
            let mesh = dom.new_instance("SpecialMesh", "Mesh", Some(part));
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
            ("face", FACE)
        }
        "Torso" => ("roblox", ""),
        _ => return,
    };
    let node = dom.new_instance("Decal", decal.0, Some(part));
    set(
        dom,
        node,
        "Texture",
        Variant::Content(Content::Uri(decal.1.into())),
    );
    set(dom, node, "Face", Variant::Enum(FRONT));
}

/// A part's attachments: the `<Joint>RigAttachment`s its joints meet at
/// first, then the accessory ones, then everything a `MeshPart` carries.
fn rig_attachments(
    dom: &mut WeakDom,
    part: Ref,
    spec: &PartSpec,
    layout: &Layout,
    rig_nodes: &mut BTreeMap<(String, String), Ref>,
) {
    let tracked = spec.mesh.is_some();
    if layout.rig_attachments {
        for joint in &layout.joints {
            let cf = if joint.part0 == spec.name {
                &joint.c0
            } else if joint.part1 == spec.name {
                &joint.c1
            } else {
                continue;
            };
            let node = attachment(
                dom,
                part,
                &format!("{}RigAttachment", joint.name),
                cf,
                tracked,
            );
            rig_nodes.insert((spec.name.clone(), joint.name.clone()), node);
        }
    }
    for (name, cf) in &spec.attachments {
        attachment(dom, part, name, cf, tracked);
    }
    if let Some(piece) = &spec.mesh {
        dress_mesh(dom, part, &spec.name, piece);
    }
}

/// An `Attachment`; on a `MeshPart` it also remembers its authored position.
fn attachment(dom: &mut WeakDom, part: Ref, name: &str, cf: &Cf, remember: bool) -> Ref {
    let node = dom.new_instance("Attachment", name, Some(part));
    set(dom, node, "CFrame", Variant::CFrame(cf.data()));
    if remember {
        let original = dom.new_instance("Vector3Value", "OriginalPosition", Some(node));
        set(dom, original, "Value", Variant::Vector3(vec3(cf.p)));
    }
    node
}

/// What makes a `MeshPart` the current-generation body part: its mesh, native
/// size, surface maps, and the scale type and cage layered clothing wraps to.
fn dress_mesh(dom: &mut WeakDom, part: Ref, name: &str, piece: &Piece) {
    let mannequin = piece.family == Family::Mannequin;
    set(dom, part, "MeshId", asset(piece.mesh));
    set(dom, part, "InitialSize", Variant::Vector3(vec3(piece.init)));
    set(
        dom,
        part,
        "RenderFidelity",
        Variant::Enum(u32::from(!mannequin)),
    );
    if let Some(texture) = piece.texture {
        set(dom, part, "TextureID", asset(texture));
    }
    if mannequin || name != "Head" {
        let scale_type = dom.new_instance("StringValue", "AvatarPartScaleType", Some(part));
        let kind = if mannequin {
            "ProportionsNormal"
        } else {
            "Classic"
        };
        set(dom, scale_type, "Value", Variant::String(kind.into()));
    }
    let original = dom.new_instance("Vector3Value", "OriginalSize", Some(part));
    set(
        dom,
        original,
        "Value",
        Variant::Vector3(vec3(piece.original)),
    );
    if let Some((cage, origin)) = piece.cage {
        let target_name = if mannequin {
            format!("{name}WrapTarget")
        } else {
            name.to_string()
        };
        let target = dom.new_instance("WrapTarget", &target_name, Some(part));
        set(dom, target, "CageMeshId", asset(cage));
        set(
            dom,
            target,
            "CageOrigin",
            Variant::CFrame(Cf::at(origin).data()),
        );
    }
    if let Some(maps) = piece.surface {
        let surface = dom.new_instance("SurfaceAppearance", "SurfaceAppearance", Some(part));
        let names = [
            "ColorMap",
            "MetalnessMap",
            "NormalMap",
            "RoughnessMap",
            "TexturePack",
        ];
        for (property, id) in names.into_iter().zip(maps) {
            set(dom, surface, property, asset(id));
        }
    }
    if name == "Head" {
        dom.new_instance("FaceControls", "FaceControls", Some(part));
        if piece.face {
            let face = dom.new_instance("Decal", "face", Some(part));
            set(dom, face, "Texture", Variant::Content(Content::Uri(FACE.into())));
            set(dom, face, "Face", Variant::Enum(FRONT));
        }
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

/// The `NoCollisionConstraint`s that keep a character's limbs from fighting
/// its torso, each living in the first part of the pair.
fn no_collisions(dom: &mut WeakDom, parts: &BTreeMap<&str, Ref>) {
    for (host, others) in NO_COLLISION {
        let Some(&part0) = parts.get(host) else {
            continue;
        };
        for other in others {
            let Some(&part1) = parts.get(other) else {
                continue;
            };
            let node = dom.new_instance(
                "NoCollisionConstraint",
                &format!("{other}NoCollision"),
                Some(part0),
            );
            set(dom, node, "Part0", Variant::Ref(part0));
            set(dom, node, "Part1", Variant::Ref(part1));
        }
    }
}

/// An R15 joint the way Studio's upgraded rigs make it: the
/// `AnimationConstraint` animation drives, and the `BallSocketConstraint`
/// that holds the limb to its range when the character is ragdolled.
fn constraints(dom: &mut WeakDom, host: Ref, joint: &str, a0: Ref, a1: Ref) {
    let drive = dom.new_instance("AnimationConstraint", joint, Some(host));
    set(dom, drive, "Attachment0", Variant::Ref(a0));
    set(dom, drive, "Attachment1", Variant::Ref(a1));
    set(dom, drive, "IsKinematic", Variant::Bool(true));
    set(dom, drive, "MaxForce", Variant::Float32(0.));
    set(dom, drive, "MaxTorque", Variant::Float32(3000.));
    for property in [
        "AngularDamping",
        "AngularStrength",
        "LinearDamping",
        "LinearStrength",
    ] {
        set(dom, drive, property, Variant::Float32(1.));
    }
    let kind = joint.trim_start_matches("Left").trim_start_matches("Right");
    let Some(&(_, cone, low, high)) = SOCKET_LIMITS.iter().find(|(name, ..)| *name == kind) else {
        return;
    };
    let socket = dom.new_instance(
        "BallSocketConstraint",
        &format!("{joint}BallSocket"),
        Some(host),
    );
    set(dom, socket, "Attachment0", Variant::Ref(a0));
    set(dom, socket, "Attachment1", Variant::Ref(a1));
    set(dom, socket, "LimitsEnabled", Variant::Bool(true));
    set(dom, socket, "TwistLimitsEnabled", Variant::Bool(true));
    set(dom, socket, "UpperAngle", Variant::Float32(cone));
    set(dom, socket, "TwistLowerAngle", Variant::Float32(low));
    set(dom, socket, "TwistUpperAngle", Variant::Float32(high));
    set(dom, socket, "Radius", Variant::Float32(0.15));
    set(dom, socket, "Restitution", Variant::Float32(0.));
}

fn humanoid(dom: &mut WeakDom, model: Ref, rig: RigType, layout: &Layout, scales: &Scales) {
    let humanoid = dom.new_instance("Humanoid", "Humanoid", Some(model));
    let (kind, hip) = match rig {
        RigType::R6 => (HUMANOID_R6, 0.),
        RigType::R15 => (HUMANOID_R15, layout.hip_height),
    };
    set(dom, humanoid, "RigType", Variant::Enum(kind));
    set(dom, humanoid, "HipHeight", Variant::Float32(hip));
    let unit = Scales {
        depth: 1.,
        height: 1.,
        width: 1.,
        head: 1.,
        body_type: 0.,
        proportion: 0.,
    };
    let scales = if rig == RigType::R15 { scales } else { &unit };
    set(
        dom,
        humanoid,
        "InternalBodyScale",
        Variant::Vector3(vec3([scales.width, scales.height, scales.depth])),
    );
    set(
        dom,
        humanoid,
        "InternalHeadScale",
        Variant::Float32(scales.head),
    );
    dom.new_instance("Animator", "Animator", Some(humanoid));
    if rig == RigType::R15 {
        for (name, value) in [
            ("BodyTypeScale", scales.body_type),
            ("BodyProportionScale", scales.proportion),
            ("BodyWidthScale", scales.width),
            ("BodyHeightScale", scales.height),
            ("BodyDepthScale", scales.depth),
            ("HeadScale", scales.head),
        ] {
            let number = dom.new_instance("NumberValue", name, Some(humanoid));
            set(dom, number, "Value", Variant::Float64(f64::from(value)));
        }
    }
    description(dom, humanoid, scales);
}

/// The `HumanoidDescription` every reference rig carries: its scales and an
/// empty slot for each body part.
fn description(dom: &mut WeakDom, humanoid: Ref, scales: &Scales) {
    let node = dom.new_instance("HumanoidDescription", "HumanoidDescription", Some(humanoid));
    for (property, value) in [
        ("BodyTypeScale", scales.body_type),
        ("ProportionScale", scales.proportion),
        ("WidthScale", scales.width),
        ("HeightScale", scales.height),
        ("DepthScale", scales.depth),
        ("HeadScale", scales.head),
    ] {
        set(dom, node, property, Variant::Float32(value));
    }
    for slot in 0..BODY_PARTS {
        let part = dom.new_instance("BodyPartDescription", "BodyPartDescription", Some(node));
        set(dom, part, "BodyPart", Variant::Enum(slot));
        set(
            dom,
            part,
            "Color",
            Variant::Color3(Color3Data {
                r: 0.5,
                g: 0.5,
                b: 0.5,
            }),
        );
    }
}
