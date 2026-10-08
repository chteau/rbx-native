use rbx_dom::{Ref, Variant, WeakDom};

use super::cframe::Cf;
use super::*;

const R6_PARTS: [&str; 7] = [
    "HumanoidRootPart",
    "Torso",
    "Head",
    "Left Arm",
    "Right Arm",
    "Left Leg",
    "Right Leg",
];

const SHAPES: [BodyShape; 2] = [BodyShape::Masculine, BodyShape::Feminine];
const SCALES: [BodyScale; 3] = [
    BodyScale::Classic,
    BodyScale::RthroNormal,
    BodyScale::RthroSlender,
];
const STYLES: [JointStyle; 2] = [JointStyle::AnimationConstraint, JointStyle::Motor6D];

fn place() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    (dom, workspace)
}

fn build(options: &RigOptions) -> (WeakDom, Ref) {
    let (mut dom, workspace) = place();
    let rig = build_rig(&mut dom, options, workspace);
    (dom, rig)
}

fn child(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.name() == name))
}

fn children_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Vec<Ref> {
    dom.get(parent)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| dom.get(c).is_some_and(|i| i.class() == class))
        .collect()
}

fn size(dom: &WeakDom, part: Ref) -> [f32; 3] {
    let Some(Variant::Vector3(v)) = dom.get(part).unwrap().properties().get("size") else {
        panic!("part without a size");
    };
    [v.x, v.y, v.z]
}

fn cframe(dom: &WeakDom, node: Ref, property: &str) -> Cf {
    let Some(Variant::CFrame(c)) = dom.get(node).unwrap().properties().get(property) else {
        panic!("no {property}");
    };
    Cf::from_data(c)
}

fn cf(dom: &WeakDom, node: Ref) -> Cf {
    cframe(dom, node, "CFrame")
}

fn reference(dom: &WeakDom, node: Ref, property: &str) -> Ref {
    let Some(Variant::Ref(r)) = dom.get(node).unwrap().properties().get(property) else {
        panic!("no {property}");
    };
    *r
}

fn near(a: &Cf, b: &Cf) -> bool {
    a.p.iter().zip(&b.p).all(|(x, y)| (x - y).abs() < 1e-3)
        && a.r.iter().zip(&b.r).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn every_combination() -> Vec<RigOptions> {
    let mut all = Vec::new();
    for rig in [RigType::R6, RigType::R15] {
        for shape in SHAPES {
            for scale in SCALES {
                for joints in STYLES {
                    all.push(RigOptions::new(rig, shape, scale, joints));
                }
            }
        }
    }
    all
}

fn r15_names() -> Vec<&'static str> {
    r15::PARTS.to_vec()
}

#[test]
fn every_rig_has_every_part_and_a_primary_part() {
    for options in every_combination() {
        let (dom, rig) = build(&options);
        let expected: Vec<&str> = match options.rig_type {
            RigType::R6 => R6_PARTS.to_vec(),
            RigType::R15 => r15_names(),
        };
        for name in &expected {
            assert!(child(&dom, rig, name).is_some(), "{name} missing");
        }
        assert_eq!(children_of_class(&dom, rig, "Part").len(), expected.len());
        assert_eq!(
            reference(&dom, rig, "PrimaryPart"),
            child(&dom, rig, "HumanoidRootPart").unwrap()
        );
        assert_eq!(dom.get(rig).unwrap().class(), "Model");
    }
}

#[test]
fn humanoid_matches_the_rig() {
    for options in every_combination() {
        let (dom, rig) = build(&options);
        let humanoid = child(&dom, rig, "Humanoid").expect("a Humanoid");
        let props = dom.get(humanoid).unwrap().properties();
        let kind = u32::from(options.rig_type == RigType::R15);
        assert_eq!(props.get("RigType"), Some(&Variant::Enum(kind)));
        assert!(child(&dom, humanoid, "Animator").is_some());
        for name in [
            "BodyDepthScale",
            "BodyHeightScale",
            "BodyWidthScale",
            "BodyTypeScale",
            "BodyProportionScale",
            "HeadScale",
        ] {
            let present = child(&dom, humanoid, name).is_some();
            assert_eq!(present, options.rig_type == RigType::R15, "{name}");
        }
        let Some(Variant::Float32(hip)) = props.get("HipHeight") else {
            panic!("no HipHeight");
        };
        match options.rig_type {
            RigType::R6 => assert_eq!(*hip, 0.),
            RigType::R15 => assert!(*hip > 0.5, "hip height {hip}"),
        }
    }
}

#[test]
fn a_rig_has_a_face_and_body_colours_on_its_parts() {
    let mut options = RigOptions::new(
        RigType::R15,
        BodyShape::Masculine,
        BodyScale::Classic,
        JointStyle::Motor6D,
    );
    options.colors.left_arm = [10, 20, 30];
    let (dom, rig) = build(&options);
    let head = child(&dom, rig, "Head").unwrap();
    assert_eq!(children_of_class(&dom, head, "Decal").len(), 1);
    let hand = child(&dom, rig, "LeftHand").unwrap();
    assert_eq!(
        dom.get(hand).unwrap().properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 10,
            g: 20,
            b: 30
        })
    );
}

#[test]
fn joints_connect_the_right_parts_and_agree_with_the_layout() {
    for options in every_combination() {
        let (dom, rig) = build(&options);
        let r15_rig = options.rig_type == RigType::R15;
        let upgraded = r15_rig && options.joints == JointStyle::AnimationConstraint;
        let (mut motors, mut constraints) = (0, 0);
        for part in dom.get(rig).unwrap().children().to_vec() {
            for node in dom.get(part).unwrap().children().to_vec() {
                let instance = dom.get(node).unwrap();
                match instance.class() {
                    "Motor6D" => {
                        motors += 1;
                        let part0 = reference(&dom, node, "Part0");
                        let part1 = reference(&dom, node, "Part1");
                        let (c0, c1) = (cframe(&dom, node, "C0"), cframe(&dom, node, "C1"));
                        assert!(
                            near(&cf(&dom, part1), &cf(&dom, part0).joined(&c0, &c1)),
                            "{} is off its joint",
                            instance.name()
                        );
                        if r15_rig {
                            let rig_name = format!("{}RigAttachment", instance.name());
                            let a0 = child(&dom, part0, &rig_name).unwrap();
                            let a1 = child(&dom, part1, &rig_name).unwrap();
                            assert!(near(&c0, &cf(&dom, a0)) && near(&c1, &cf(&dom, a1)));
                            assert_eq!(part, part1);
                        } else {
                            assert_eq!(part, part0);
                        }
                    }
                    "AnimationConstraint" => {
                        constraints += 1;
                        let a0 = reference(&dom, node, "Attachment0");
                        let a1 = reference(&dom, node, "Attachment1");
                        let (p0, p1) = (dom.parent(a0).unwrap(), dom.parent(a1).unwrap());
                        assert_eq!(p1, part);
                        let world0 = cf(&dom, p0).mul(&cf(&dom, a0));
                        assert!(near(&world0, &cf(&dom, p1).mul(&cf(&dom, a1))));
                        assert_eq!(
                            dom.get(a0).unwrap().name(),
                            format!("{}RigAttachment", instance.name())
                        );
                    }
                    _ => {}
                }
            }
        }
        match (options.rig_type, upgraded) {
            (RigType::R6, _) => assert_eq!((motors, constraints), (6, 0)),
            (RigType::R15, true) => assert_eq!((motors, constraints), (0, 15)),
            (RigType::R15, false) => assert_eq!((motors, constraints), (15, 0)),
        }
    }
}

#[test]
fn r6_uses_studios_exact_motor_matrices() {
    let options = RigOptions::new(
        RigType::R6,
        BodyShape::Masculine,
        BodyScale::Classic,
        JointStyle::AnimationConstraint,
    );
    let (dom, rig) = build(&options);
    let torso = child(&dom, rig, "Torso").unwrap();
    let neck = child(&dom, torso, "Neck").unwrap();
    assert_eq!(cframe(&dom, neck, "C0").p, [0., 1., 0.]);
    assert_eq!(cframe(&dom, neck, "C1").p, [0., -0.5, 0.]);
    let right = child(&dom, torso, "Right Shoulder").unwrap();
    assert_eq!(
        cframe(&dom, right, "C0").r,
        [0., 0., 1., 0., 1., 0., -1., 0., 0.]
    );
    let left = child(&dom, torso, "Left Hip").unwrap();
    assert_eq!(cframe(&dom, left, "C1").p, [-0.5, 1., 0.]);
}

#[test]
fn the_rig_stands_with_its_feet_on_the_spawn_point() {
    for mut options in every_combination() {
        options.feet = [4., 10., -6.];
        let (dom, rig) = build(&options);
        let bottom = dom
            .get(rig)
            .unwrap()
            .children()
            .iter()
            .filter(|&&c| dom.get(c).unwrap().class() == "Part")
            .filter(|&&c| dom.get(c).unwrap().name() != "HumanoidRootPart")
            .map(|&p| cf(&dom, p).p[1] - size(&dom, p)[1] / 2.)
            .fold(f32::INFINITY, f32::min);
        assert!((bottom - 10.).abs() < 1e-3, "{bottom}");
        let root = cf(&dom, child(&dom, rig, "HumanoidRootPart").unwrap());
        assert_eq!(root.r, [1., 0., 0., 0., 1., 0., 0., 0., 1.]);
        assert!((root.p[0] - 4.).abs() < 1e-4 && (root.p[2] + 6.).abs() < 1e-4);
    }
}

/// Widest x and z, summed y, of the named parts.
fn group(dom: &WeakDom, rig: Ref, names: &[&str]) -> [f32; 3] {
    let sizes: Vec<[f32; 3]> = names
        .iter()
        .map(|n| size(dom, child(dom, rig, n).unwrap()))
        .collect();
    [
        sizes.iter().map(|s| s[0]).fold(0., f32::max),
        sizes.iter().map(|s| s[1]).sum(),
        sizes.iter().map(|s| s[2]).fold(0., f32::max),
    ]
}

fn within(actual: [f32; 3], min: [f32; 3], max: [f32; 3], what: &str) {
    for i in 0..3 {
        assert!(
            actual[i] >= min[i] - 1e-3 && actual[i] <= max[i] + 1e-3,
            "{what}: {actual:?} outside {min:?}..{max:?}"
        );
    }
}

#[test]
fn r15_sizes_stay_inside_the_documented_limits() {
    for shape in SHAPES {
        for scale in SCALES {
            let options = RigOptions::new(RigType::R15, shape, scale, JointStyle::Motor6D);
            let (dom, rig) = build(&options);
            // Classic's blocky head is 2x1x1, wider than the Classic row's
            // 1.5: it is checked against the Normal row (see `proportions`).
            let (head, arm, torso, leg) = match scale {
                BodyScale::Classic | BodyScale::RthroNormal => (
                    [3., 2., 2.],
                    if scale == BodyScale::Classic {
                        [2., 3., 2.]
                    } else {
                        [2., 4.5, 2.]
                    },
                    if scale == BodyScale::Classic {
                        [4., 3.8, 2.]
                    } else {
                        [4.6, 3.5, 2.25]
                    },
                    if scale == BodyScale::Classic {
                        [1.5, 3.5, 2.]
                    } else {
                        [1.5, 4., 2.]
                    },
                ),
                BodyScale::RthroSlender => {
                    ([2., 2., 2.], [1.5, 4., 2.], [3., 3.5, 2.], [1.5, 4., 2.])
                }
            };
            let what = format!("{shape:?} {scale:?}");
            within(group(&dom, rig, &["Head"]), [0.5; 3], head, &what);
            let arm_size = group(&dom, rig, &["LeftUpperArm", "LeftLowerArm", "LeftHand"]);
            within(arm_size, [0.25, 1.5, 0.25], arm, &what);
            let torso_size = group(&dom, rig, &["UpperTorso", "LowerTorso"]);
            within(torso_size, [0.85, 1.7, 0.7], torso, &what);
            let leg_size = group(&dom, rig, &["RightUpperLeg", "RightLowerLeg", "RightFoot"]);
            within(leg_size, [0.25, 1.4, 0.5], leg, &what);
        }
    }
}

#[test]
fn every_scale_and_shape_gives_a_different_body() {
    let mut seen = std::collections::HashSet::new();
    for shape in SHAPES {
        for scale in SCALES {
            let options = RigOptions::new(RigType::R15, shape, scale, JointStyle::Motor6D);
            let (dom, rig) = build(&options);
            let key: Vec<i32> = r15_names()
                .iter()
                .flat_map(|n| size(&dom, child(&dom, rig, n).unwrap()))
                .map(|v| (v * 1000.) as i32)
                .collect();
            assert!(seen.insert(key), "{shape:?} {scale:?} repeats another body");
        }
    }
}

fn torso(shape: BodyShape, scale: BodyScale) -> [f32; 3] {
    let options = RigOptions::new(RigType::R15, shape, scale, JointStyle::Motor6D);
    let (dom, rig) = build(&options);
    group(&dom, rig, &["UpperTorso"])
}

#[test]
fn torso_width_shrinks_from_classic_to_normal_to_slender_and_for_feminine() {
    let width = |scale| torso(BodyShape::Masculine, scale)[0];
    assert!(width(BodyScale::RthroSlender) < width(BodyScale::RthroNormal));
    assert!(width(BodyScale::RthroNormal) < width(BodyScale::Classic));
    for scale in SCALES {
        let (m, f) = (
            torso(BodyShape::Masculine, scale),
            torso(BodyShape::Feminine, scale),
        );
        assert!(f[0] < m[0] && f[1] < m[1], "{scale:?}");
    }
}

#[test]
fn a_second_rig_gets_a_unique_name() {
    let (mut dom, workspace) = place();
    let options = RigOptions::new(
        RigType::R6,
        BodyShape::Masculine,
        BodyScale::Classic,
        JointStyle::Motor6D,
    );
    let first = build_rig(&mut dom, &options, workspace);
    let second = build_rig(&mut dom, &options, workspace);
    assert_eq!(dom.get(first).unwrap().name(), "Rig");
    assert_eq!(dom.get(second).unwrap().name(), "Rig1");
}

#[test]
fn the_joint_style_follows_the_place_setting() {
    let mut dom = WeakDom::new();
    assert_eq!(JointStyle::of_place(&dom), JointStyle::AnimationConstraint);
    let player = dom.new_instance("StarterPlayer", "StarterPlayer", None);
    assert_eq!(JointStyle::of_place(&dom), JointStyle::AnimationConstraint);
    for (value, style) in [
        (1, JointStyle::Motor6D),
        (2, JointStyle::AnimationConstraint),
        (0, JointStyle::AnimationConstraint),
    ] {
        dom.set_property(
            player,
            "AvatarJointUpgrade_SerializedRollout",
            Variant::Enum(value),
        )
        .unwrap();
        assert_eq!(JointStyle::of_place(&dom), style);
    }
}

fn count_tree(dom: &WeakDom, node: Ref) -> usize {
    1 + dom
        .get(node)
        .unwrap()
        .children()
        .iter()
        .map(|&c| count_tree(dom, c))
        .sum::<usize>()
}

/// Joint endpoints, as `(joint name, Part0 name, Part1 name)` for motors and
/// attachment owners for constraints, plus every part's size and CFrame.
fn fingerprint(dom: &WeakDom, rig: Ref) -> Vec<String> {
    let name = |r: Ref| dom.get(r).unwrap().name().to_string();
    let mut lines = Vec::new();
    for part in dom.get(rig).unwrap().children().to_vec() {
        let instance = dom.get(part).unwrap();
        if instance.class() == "Part" {
            lines.push(format!(
                "{} {:?} {:?}",
                instance.name(),
                size(dom, part),
                cf(dom, part).p
            ));
        }
        for node in instance.children().to_vec() {
            match dom.get(node).unwrap().class() {
                "Motor6D" => lines.push(format!(
                    "{} {} {}",
                    name(node),
                    name(reference(dom, node, "Part0")),
                    name(reference(dom, node, "Part1"))
                )),
                "AnimationConstraint" => lines.push(format!(
                    "{} {} {}",
                    name(node),
                    name(dom.parent(reference(dom, node, "Attachment0")).unwrap()),
                    name(dom.parent(reference(dom, node, "Attachment1")).unwrap())
                )),
                _ => {}
            }
        }
    }
    lines
}

#[test]
fn an_inserted_rig_survives_both_save_formats() {
    for options in every_combination() {
        let (dom, rig) = build(&options);
        let (count, expected) = (count_tree(&dom, rig), fingerprint(&dom, rig));
        let binary = rbx_binary::serialize(&dom).expect("binary writes");
        let back = rbx_binary::deserialize(&binary).expect("binary reads");
        let workspace = back.root_refs()[0];
        let rig = child(&back, workspace, "Rig").expect("the rig is back");
        assert_eq!(count_tree(&back, rig), count);
        assert_eq!(fingerprint(&back, rig), expected);

        let xml = rbx_xml::serialize(&dom).expect("xml writes");
        let back = rbx_xml::deserialize(&xml).expect("xml reads");
        let workspace = back.root_refs()[0];
        let rig = child(&back, workspace, "Rig").expect("the rig is back");
        assert_eq!(count_tree(&back, rig), count);
        assert_eq!(fingerprint(&back, rig), expected);
    }
}
