mod support;

use rbx_binary::{deserialize, parse_header};
use rbx_dom::{Ref, Variant, WeakDom};

use support::{all_refs, determinant, instances_of, property, FPS, TEST_PLACE};

fn is_ancestor(dom: &WeakDom, ancestor: Ref, descendant: Ref) -> bool {
    let Some(instance) = dom.get(ancestor) else {
        return false;
    };

    instance
        .children()
        .iter()
        .any(|&child| child == descendant || is_ancestor(dom, child, descendant))
}

#[test]
fn deserializes_both_files_with_the_announced_instance_count() {
    for bytes in [FPS, TEST_PLACE] {
        let (header, _) = parse_header(bytes).unwrap();
        let dom = deserialize(bytes).unwrap();

        assert_eq!(all_refs(&dom).len() as i32, header.num_instances);
    }
}

#[test]
fn fps_referents_are_zero_to_forty_seven_without_holes() {
    let dom = deserialize(FPS).unwrap();

    let mut ids: Vec<u32> = all_refs(&dom).iter().map(Ref::value).collect();
    ids.sort_unstable();

    assert_eq!(ids, (0..48).collect::<Vec<u32>>());
}

#[test]
fn fps_has_a_single_root() {
    let dom = deserialize(FPS).unwrap();
    assert_eq!(dom.root_refs().len(), 1);
}

#[test]
fn motor6d_joints_point_at_parts() {
    let dom = deserialize(FPS).unwrap();
    let motors = instances_of(&dom, "Motor6D");
    assert_eq!(motors.len(), 2);

    let targets: Vec<Ref> = motors
        .iter()
        .flat_map(|motor| ["Part0", "Part1"].map(|name| property(motor, name).clone()))
        .map(|value| match value {
            Variant::Ref(referent) => referent,
            other => panic!("expected a Ref, got {other:?}"),
        })
        .collect();

    assert_eq!(
        targets.iter().map(|r| r.value()).collect::<Vec<u32>>(),
        vec![42, 45, 42, 47]
    );
    for target in targets {
        assert_eq!(dom.get(target).unwrap().class(), "Part");
    }
}

#[test]
fn part_material_is_smooth_plastic() {
    let dom = deserialize(FPS).unwrap();
    let parts = instances_of(&dom, "Part");
    assert_eq!(parts.len(), 5);

    for part in parts {
        assert_eq!(property(part, "Material"), &Variant::Enum(272));
    }
}

#[test]
fn part_sizes_and_transparency_decode_to_studs() {
    let dom = deserialize(FPS).unwrap();
    let parts = instances_of(&dom, "Part");

    let sizes: Vec<(f32, f32, f32)> = parts
        .iter()
        .map(|part| match property(part, "size") {
            Variant::Vector3(v) => (v.x, v.y, v.z),
            other => panic!("expected a Vector3, got {other:?}"),
        })
        .collect();

    let expected = [
        (0.3, 0.3, 1.5),
        (0.3, 0.3, 1.1),
        (0.2, 0.2, 0.2),
        (1.0, 1.0, 2.5),
        (1.0, 1.0, 2.5),
    ];
    for (actual, expected) in sizes.iter().zip(expected) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-6
                && (actual.1 - expected.1).abs() < 1e-6
                && (actual.2 - expected.2).abs() < 1e-6,
            "size {actual:?} != {expected:?}"
        );
    }

    let transparency: Vec<f32> = parts
        .iter()
        .map(|part| match property(part, "Transparency") {
            Variant::Float32(value) => *value,
            other => panic!("expected a Float32, got {other:?}"),
        })
        .collect();
    assert_eq!(transparency, vec![0.0, 0.0, 1.0, 0.0, 0.0]);
}

#[test]
fn part_colors_are_byte_planes_and_both_arms_share_a_skin_tone() {
    let dom = deserialize(FPS).unwrap();
    let parts = instances_of(&dom, "Part");

    let colors: Vec<(u8, u8, u8)> = parts
        .iter()
        .map(|part| match property(part, "Color3uint8") {
            Variant::Color3uint8 { r, g, b } => (*r, *g, *b),
            other => panic!("expected a Color3uint8, got {other:?}"),
        })
        .collect();

    assert_eq!(
        colors,
        vec![
            (180, 190, 200),
            (40, 200, 160),
            (255, 255, 255),
            (225, 190, 160),
            (225, 190, 160),
        ]
    );

    // RightArm and LeftArm are the last two Parts and must share one skin tone;
    // this is what rules out the "one RGB triple per instance" layout.
    let names: Vec<&str> = parts.iter().map(|part| part.name()).collect();
    assert_eq!(names[3..], ["RightArm", "LeftArm"]);
    assert_eq!(colors[3], colors[4]);
}

#[test]
fn part_cframes_mix_compact_and_raw_rotations() {
    let dom = deserialize(FPS).unwrap();
    let parts = instances_of(&dom, "Part");

    let frames: Vec<([f32; 9], (f32, f32, f32))> = parts
        .iter()
        .map(|part| match property(part, "CFrame") {
            Variant::CFrame(frame) => (
                frame.rotation,
                (frame.position.x, frame.position.y, frame.position.z),
            ),
            other => panic!("expected a CFrame, got {other:?}"),
        })
        .collect();

    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    // The two knife handles use the compact rotation id 0x02 (identity) and sit
    // where the tools were dropped; the three body parts store raw matrices.
    assert_eq!(frames[0].0, identity);
    assert_eq!(frames[1].0, identity);
    assert_eq!(frames[0].1, (0.0, 17.0, 0.0));
    assert_eq!(frames[1].1, (0.0, 13.0, 0.0));
    assert_ne!(frames[2].0, identity);

    for (rotation, _) in &frames {
        let det = determinant(rotation);
        assert!(
            (det - 1.0).abs() < 1e-4,
            "rotation is not a rotation: {det}"
        );
    }
}

#[test]
fn fps_names_come_from_the_name_property() {
    let dom = deserialize(FPS).unwrap();

    let tools = instances_of(&dom, "Tool");
    assert_eq!(
        tools.iter().map(|tool| tool.name()).collect::<Vec<&str>>(),
        vec!["Knife", "Knife"]
    );
    // Name is redirected to Instance::name instead of being duplicated.
    assert!(tools[0].properties().get("Name").is_none());
}

#[test]
fn test_place_nests_the_baseplate_and_spawn_under_workspace() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let workspace = instances_of(&dom, "Workspace");
    assert_eq!(workspace.len(), 1);
    assert_eq!(workspace[0].name(), "Workspace");

    for class in ["Part", "SpawnLocation"] {
        let instances = instances_of(&dom, class);
        assert_eq!(instances.len(), 1, "expected exactly one {class}");
        assert!(
            is_ancestor(&dom, workspace[0].referent(), instances[0].referent()),
            "{class} is not a descendant of Workspace"
        );
    }
}

#[test]
fn test_place_services_sit_at_the_root() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let lighting = instances_of(&dom, "Lighting");
    assert_eq!(lighting.len(), 1);
    // Services are parented to -1 in PRNT, which the DOM represents as a root.
    assert!(dom.root_refs().contains(&lighting[0].referent()));
    // 3 is Enum.Technology.ShadowMap, the default of a fresh baseplate place.
    assert_eq!(property(lighting[0], "Technology"), &Variant::Enum(3));
}

// The parser must survive arbitrary damage: every result below is allowed to be
// an error, none is allowed to panic, hang or allocate the machine to death.
#[test]
fn truncated_files_never_panic() {
    for bytes in [FPS, TEST_PLACE] {
        for len in (0..bytes.len()).step_by(89) {
            let _ = deserialize(&bytes[..len]);
        }
        let _ = deserialize(bytes);
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    for bytes in [FPS, TEST_PLACE] {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..400 {
            let mut damaged = bytes.to_vec();
            for _ in 0..8 {
                // xorshift* keeps the mutation deterministic without a dev-dependency.
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let index =
                    (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize % damaged.len();
                damaged[index] = (state >> 8) as u8;
            }
            let _ = deserialize(&damaged);
        }
    }
}
