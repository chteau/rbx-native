use crate::test_humanoid as humanoid;

mod real;
mod skin;

use glam::Vec3;

use crate::{fit, mirror, split, Axis, Fit, Joint, Orientation, Part, Side};
use humanoid::{humanoid, to_file, Humanoid, Pose};

const HEIGHT: f32 = 5.2;

/// The synthetic body's joints where the fit should put them: scaled to
/// `HEIGHT`, with z measured from the centre of the bounds like the fit's.
fn expected(body: &Humanoid) -> [Vec3; 15] {
    let (lo, hi) = crate::mesh::bounds(body.mesh.positions.iter().copied());
    let zc = (lo.z + hi.z) / 2.;
    body.joints.map(|p| Vec3::new(p.x, p.y, p.z - zc) * HEIGHT)
}

const ARM_JOINTS: [Joint; 6] = [
    Joint::LeftShoulder,
    Joint::LeftElbow,
    Joint::LeftWrist,
    Joint::RightShoulder,
    Joint::RightElbow,
    Joint::RightWrist,
];

/// One joint's distance from where it should be, as a fraction of the height.
fn joint_error(fit: &Fit, body: &Humanoid, joint: Joint) -> f32 {
    (fit.joint(joint) - expected(body)[joint as usize]).length() / HEIGHT
}

/// Fails with every joint's error listed if any is past `tolerance`·height.
fn assert_joints(fit: &Fit, body: &Humanoid, tolerance: f32) {
    let want = expected(body);
    let mut report = String::new();
    let mut worst = 0f32;
    for joint in Joint::ALL {
        let error = (fit.joint(joint) - want[joint as usize]).length() / HEIGHT;
        worst = worst.max(error);
        report += &format!(
            "{:>14}: got {:6.3?} want {:6.3?} error {:.3}·H\n",
            joint.name(),
            fit.joint(joint).to_array(),
            want[joint as usize].to_array(),
            error
        );
    }
    eprintln!("{report}warnings: {:?}", fit.warnings);
    assert!(
        worst <= tolerance,
        "worst joint error {worst:.3}·H\n{report}"
    );
}

fn check_fit(pose: Pose, orientation: Orientation, units: f32) -> (Humanoid, crate::Mesh, Fit) {
    check_fit_of(humanoid(pose), orientation, units, 0.05)
}

fn check_fit_of(
    body: Humanoid,
    orientation: Orientation,
    units: f32,
    tolerance: f32,
) -> (Humanoid, crate::Mesh, Fit) {
    let file = to_file(&body.mesh, orientation, units);
    let fit = fit(&file, HEIGHT).unwrap();
    assert_eq!(fit.orientation, orientation);
    // Measured to the head's top voxel when the hands are higher.
    let slack = if fit.warnings.iter().any(|w| w.contains("above the head")) {
        0.01
    } else {
        1e-3
    };
    let ratio = fit.scale * units / HEIGHT;
    assert!((ratio - 1.).abs() < slack, "scale off by {ratio}");
    assert_joints(&fit, &body, tolerance);
    // Left is −X in Roblox space.
    assert!(fit.joint(Joint::LeftWrist).x < 0. && fit.joint(Joint::RightWrist).x > 0.);
    assert!(fit.extremities.left_foot.x < 0. && fit.extremities.right_hand.x > 0.);
    (body, file, fit)
}

#[test]
fn t_pose_in_roblox_space() {
    check_fit(Pose::T, Orientation::ROBLOX, 1.);
}

#[test]
fn a_pose_z_up_facing_minus_y() {
    // Blender's convention: Z up, the character facing −Y.
    check_fit(
        Pose::A,
        Orientation::new(Axis::PosZ, Axis::NegY).unwrap(),
        1.,
    );
}

#[test]
fn i_pose_facing_plus_z_in_centimetres() {
    check_fit(
        Pose::I,
        Orientation::new(Axis::PosY, Axis::PosZ).unwrap(),
        100. / HEIGHT * 1.8,
    );
}

/// Every edge of `mesh` (vertices merged by exact position) is used by
/// exactly two triangles, once in each direction: closed and consistently
/// wound.
fn assert_watertight(mesh: &crate::Mesh, what: &str) {
    use std::collections::HashMap;
    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
    let id: Vec<u32> = mesh
        .positions
        .iter()
        .map(|p| {
            let n = ids.len() as u32;
            *ids.entry(p.to_array().map(f32::to_bits)).or_insert(n)
        })
        .collect();
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &mesh.triangles {
        for k in 0..3 {
            let (a, b) = (id[t[k] as usize], id[t[(k + 1) % 3] as usize]);
            assert_ne!(a, b, "{what}: degenerate edge");
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    let bad: Vec<_> = directed
        .iter()
        .filter(|(&(a, b), &n)| n != 1 || directed.get(&(b, a)) != Some(&1))
        .take(5)
        .collect();

    assert!(
        bad.is_empty(),
        "{what}: {} of {} edges are open or doubled, e.g. {bad:?}",
        directed
            .iter()
            .filter(|(&(a, b), &n)| n != 1 || directed.get(&(b, a)) != Some(&1))
            .count(),
        directed.len()
    );
}

/// The pose's body, fit and split, checked once and shared between tests.
fn check_split(pose: Pose) -> &'static (Humanoid, Fit, crate::Body) {
    use std::sync::OnceLock;
    static SPLITS: [OnceLock<(Humanoid, Fit, crate::Body)>; 5] = [const { OnceLock::new() }; 5];
    SPLITS[pose as usize].get_or_init(|| {
        let body = humanoid(pose);
        if pose == Pose::Pressed {
            // The fit traces an arm lying along the flank into the torso
            // (shoulders at the hips): this pose tests what follows the fit,
            // on the true joints.
            let mut fit = fit(&body.mesh, HEIGHT).unwrap();
            for joint in Joint::ALL {
                fit.set_joint(joint, expected(&body)[joint as usize]);
            }
            return checked_split_with(body, fit);
        }
        checked_split(body)
    })
}

fn checked_split(body: Humanoid) -> (Humanoid, Fit, crate::Body) {
    let fit = fit(&body.mesh, HEIGHT).unwrap();
    checked_split_with(body, fit)
}

fn checked_split_with(body: Humanoid, fit: Fit) -> (Humanoid, Fit, crate::Body) {
    let parts = split(&body.mesh, &fit).unwrap();
    assert_eq!(parts.parts.len(), 15);
    for shell in &body.shells {
        assert_watertight(shell, "input shell");
    }
    // Splitting a triangle on a plane makes three; this coarse body (12
    // segments round a limb, several shells crossing most cuts) is about the
    // worst case for that.
    let input = body.mesh.triangles.len();
    let caps: usize = parts.parts.iter().map(|p| p.cap_triangles).sum();
    let cut = parts.triangle_count() - caps;
    assert!(
        cut >= input && cut <= input + input * 4 / 10,
        "{input} triangles in, {cut} after cutting"
    );
    assert!(caps < input / 3, "{caps} cap triangles");
    for (piece, part) in parts.parts.iter().zip(Part::ALL) {
        assert_eq!(piece.part, part);
        assert!(!piece.mesh.triangles.is_empty(), "{part} is empty");
        assert_watertight(&piece.mesh, part.name());
        assert_eq!(piece.mesh.normals.len(), piece.mesh.positions.len());
        assert_eq!(piece.mesh.uvs.len(), piece.mesh.positions.len());
        // Recentred on its bounds.
        let (lo, hi) = crate::mesh::bounds(piece.mesh.positions.iter().copied());
        assert!(((lo + hi) / 2.).length() < 1e-4, "{part} is not centred");
        assert!((hi - lo - piece.size).length() < 1e-4);
        // Rig attachments: the R15 table, each at its joint in world space
        // and on (or very near) the part.
        let mut want: Vec<String> = Joint::ALL
            .iter()
            .filter(|j| j.part() == part || j.parent_part() == Some(part))
            .map(|j| j.attachment())
            .collect();
        let mut got: Vec<String> = piece.attachments.iter().map(|a| a.0.clone()).collect();
        assert_eq!(got[0], part.joint().attachment());
        want.sort();
        got.sort();
        assert_eq!(got, want, "{part}");
        for (name, offset) in &piece.attachments {
            let joint = Joint::ALL
                .into_iter()
                .find(|j| &j.attachment() == name)
                .unwrap();
            assert!((piece.centre + *offset - fit.joint(joint)).length() < 1e-4);
            let outside = (offset.abs() - piece.size / 2.).max(Vec3::ZERO).length();
            assert!(
                outside < 0.03 * HEIGHT,
                "{part}: {name} lies {outside} studs off the part"
            );
        }
    }
    // Sides: left parts on −X.
    for part in Part::ALL {
        match part.side() {
            Some(Side::Left) => assert!(parts.part(part).centre.x < 0., "{part}"),
            Some(Side::Right) => assert!(parts.part(part).centre.x > 0., "{part}"),
            None => assert!(parts.part(part).centre.x.abs() < 0.05 * HEIGHT, "{part}"),
        }
    }
    // Distal cuts are exact planes: no hand, forearm or foot vertex lies on
    // the parent's side of its joint's cut.
    for joint in [
        Joint::LeftWrist,
        Joint::RightElbow,
        Joint::LeftKnee,
        Joint::RightAnkle,
        Joint::Neck,
    ] {
        let (point, normal) = crate::split::plane(&fit, joint);
        let piece = parts.part(joint.part());
        let worst = piece
            .mesh
            .positions
            .iter()
            .map(|p| (piece.centre + *p - point).dot(normal))
            .fold(f32::INFINITY, f32::min);
        assert!(
            worst > -1e-3,
            "{joint}: a vertex {worst} studs past the cut"
        );
    }
    (body, fit, parts)
}

#[test]
fn arms_raised_above_the_head_fit() {
    let (_, _, fit) = check_fit_of(
        humanoid(Pose::Raised),
        Orientation::new(Axis::PosZ, Axis::NegY).unwrap(),
        1.,
        0.035,
    );
    let body = humanoid(Pose::Raised);
    for joint in ARM_JOINTS {
        let error = joint_error(&fit, &body, joint);
        // The shoulders hang off the neck's height, which the raised arms
        // blur (both ~0.03·H high); elbows and wrists are on the arm itself.
        let bar = if joint.to_string().contains("Shoulder") {
            0.035
        } else {
            0.02
        };
        assert!(error <= bar, "{joint}: {error:.3}·H");
    }
    // The hands are above the head, but the height is still the head's.
    assert!(fit.extremities.left_hand.y > fit.extremities.head.y);
}

#[test]
fn arms_raised_above_the_head_split_into_fifteen_closed_parts() {
    let (_, fit, body) = check_split(Pose::Raised);
    // The upper arm's cut must not take the head along.
    let head = body.part(Part::Head);
    assert!(head.centre.y > fit.joint(Joint::Neck).y);
    assert!(head.size.y > 0.1 * HEIGHT, "head {:?}", head.size);
}

#[test]
fn hands_apart_from_their_forearms_fit() {
    for (pose, gap) in [(Pose::A, 0.02), (Pose::T, 0.04), (Pose::I, 0.02)] {
        let body = humanoid::detached_hands(pose, gap);
        // Before the gaps were bridged the fit hung the hands from the
        // forearms' ends: wrists 0.032·H off in the A-pose.
        let check = humanoid::detached_hands(pose, gap);
        let (_, _, fit) = check_fit_of(body, Orientation::ROBLOX, 1., 0.035);
        for joint in [Joint::LeftWrist, Joint::RightWrist] {
            let error = joint_error(&fit, &check, joint);
            assert!(error <= 0.015, "{pose:?} {joint}: {error:.3}·H");
        }
    }
}

#[test]
fn hands_apart_from_their_forearms_split_into_fifteen_closed_parts() {
    let (body, fit, parts) = checked_split(humanoid::detached_hands(Pose::A, 0.02));
    // Each hand is the whole detached piece, 0.09·H long.
    for side in [Side::Left, Side::Right] {
        let hand = parts.part(match side {
            Side::Left => Part::LeftHand,
            Side::Right => Part::RightHand,
        });
        assert!(
            hand.size.max_element() > 0.085 * HEIGHT,
            "{side:?} {:?}",
            hand.size
        );
    }
    let _ = (body, fit);
}

#[test]
fn an_a_pose_splits_into_fifteen_closed_parts() {
    let (_, _, body) = check_split(Pose::A);
    assert!(body.warnings.is_empty() || body.warnings.iter().all(|w| w.contains("budget")));
}

#[test]
fn a_t_pose_splits_into_fifteen_closed_parts() {
    check_split(Pose::T);
}

#[test]
fn an_i_pose_splits_into_fifteen_closed_parts() {
    check_split(Pose::I);
}

#[test]
fn mirroring_copies_one_side_onto_the_other() {
    let body = humanoid(Pose::A);
    let mut fit = fit(&body.mesh, HEIGHT).unwrap();
    let moved = fit.joint(Joint::LeftElbow) + Vec3::new(-0.2, 0.1, 0.05);
    fit.set_joint(Joint::LeftElbow, moved);
    fit.set_joint(Joint::Neck, fit.joint(Joint::Neck) + Vec3::X * 0.3);
    fit.extremities.left_hand.y += 0.1;
    mirror(&mut fit, Side::Left);
    assert_eq!(
        fit.joint(Joint::RightElbow),
        Vec3::new(-moved.x, moved.y, moved.z)
    );
    assert_eq!(fit.joint(Joint::LeftElbow), moved);
    assert_eq!(fit.joint(Joint::Neck).x, 0.);
    let (l, r) = (fit.extremities.left_hand, fit.extremities.right_hand);
    assert_eq!(r, Vec3::new(-l.x, l.y, l.z));
    for joint in Joint::ALL {
        let (a, b) = (fit.joint(joint), fit.joint(joint.mirror()));
        assert_eq!(a, Vec3::new(-b.x, b.y, b.z), "{joint}");
    }
}

#[test]
fn a_shape_without_five_limbs_is_refused() {
    // Head and legs but no arms: three tips.
    let body = humanoid(Pose::A);
    let mut armless = crate::Mesh::default();
    for (i, shell) in body.shells.iter().enumerate() {
        if !matches!(i, 3..=5 | 9..=11) {
            armless.append(shell.clone());
        }
    }
    match fit(&armless, HEIGHT) {
        Err(crate::Error::Extremities { found }) => {
            assert!(found < 5);
            let text = crate::Error::Extremities { found }.to_string();
            assert!(
                text.contains("expected 5") && text.contains("by hand"),
                "{text}"
            );
        }
        other => panic!("expected an extremity error, got {other:?}"),
    }
    assert!(matches!(
        fit(&crate::Mesh::default(), HEIGHT),
        Err(crate::Error::Empty)
    ));
    assert!(matches!(fit(&armless, -1.), Err(crate::Error::Height)));
}

#[test]
fn orientation_rotations_are_proper_and_put_the_left_on_minus_x() {
    assert_eq!(Orientation::ROBLOX.rotation(), glam::Mat3::IDENTITY);
    let blender = Orientation::new(Axis::PosZ, Axis::NegY).unwrap();
    let r = blender.rotation();
    assert!((r.determinant() - 1.).abs() < 1e-6);
    assert_eq!(r * Vec3::Z, Vec3::Y);
    assert_eq!(r * Vec3::NEG_Y, Vec3::NEG_Z);
    // Facing −Y with Z up, the character's left hand is on +X.
    assert_eq!(r * Vec3::X, Vec3::NEG_X);
    for o in Axis::ALL {
        for f in Axis::ALL {
            if let Ok(o) = Orientation::new(o, f) {
                assert!((o.rotation().determinant() - 1.).abs() < 1e-6);
                assert_eq!(o.turned(4), o);
                assert_ne!(o.turned(1), o);
                assert_eq!(o.turned(1).up, o.up);
            }
        }
    }
    assert!(Orientation::new(Axis::PosY, Axis::NegY).is_err());
}

#[test]
fn caps_close_a_loop_facing_away_from_its_winding() {
    let square = [
        Vec3::new(0., 0., 0.),
        Vec3::new(1., 0., 0.),
        Vec3::new(1., 0., 1.),
        Vec3::new(0.5, 0., 0.5), // a reflex corner
        Vec3::new(0., 0., 1.),
    ];
    let (tris, normal) = crate::split::cap(&square);
    assert_eq!(tris.len(), 3);
    assert!((normal - Vec3::NEG_Y).length() < 1e-6, "{normal}");
    let area: f32 = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| square[i]);
            (b - a).cross(c - a).dot(normal) / 2.
        })
        .sum();
    assert!((area - 0.75).abs() < 1e-5, "{area}");
}

#[test]
fn mesh_v2_parses_back_to_the_same_geometry() {
    let (_, _, body) = check_split(Pose::T);
    let piece = &body.part(Part::LeftHand).mesh;
    let bytes = crate::mesh_v2(piece);
    let parsed = rbx_mesh::parse(&bytes).unwrap();
    assert_eq!(parsed.version, (2, 0));
    assert_eq!(parsed.vertices.len(), piece.positions.len());
    for (i, v) in parsed.vertices.iter().enumerate() {
        assert_eq!(v.position, piece.positions[i].to_array());
        assert_eq!(v.normal, piece.normals[i].to_array());
        assert_eq!(v.uv, piece.uvs[i].to_array());
        assert_eq!(v.color, [255; 4]);
    }
    let indices: Vec<u32> = piece.triangles.iter().flatten().copied().collect();
    assert_eq!(parsed.indices, indices);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx_autorig_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_glb_holds_fifteen_named_parts_and_reads_back() {
    let (_, _, body) = check_split(Pose::A);
    let bytes = crate::gltf(body);
    let file = gltf::Gltf::from_slice(&bytes).unwrap();
    let mut names: Vec<String> = file
        .nodes()
        .map(|n| {
            assert!(n.mesh().is_some());
            n.name().unwrap().to_owned()
        })
        .collect();
    names.sort();
    let mut want: Vec<String> = Part::ALL
        .iter()
        .map(|p| format!("{}_Geo", p.name()))
        .collect();
    want.sort();
    assert_eq!(names, want);
    assert_eq!(file.scenes().next().unwrap().nodes().count(), 15);

    // Through `read`, as .glb and as .gltf with a separate .bin.
    let dir = scratch("glb");
    let glb = dir.join("body.glb");
    std::fs::write(&glb, &bytes).unwrap();
    let back = crate::read(&glb).unwrap();
    assert_eq!(back.triangles.len(), body.triangle_count());
    assert_eq!(back.up_hint, Some(Axis::PosY));
    let world: Vec<Vec3> = body
        .parts
        .iter()
        .flat_map(|p| p.mesh.positions.iter().map(move |q| p.centre + *q))
        .collect();
    let (lo, hi) = crate::mesh::bounds(world.iter().copied());
    let (blo, bhi) = crate::mesh::bounds(back.positions.iter().copied());
    assert!((lo - blo).length() < 1e-4 && (hi - bhi).length() < 1e-4);

    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    json["buffers"][0]["uri"] = "body.bin".into();
    std::fs::write(dir.join("body.bin"), &bytes[20 + json_len + 8..]).unwrap();
    let gltf = dir.join("body.gltf");
    std::fs::write(&gltf, serde_json::to_vec(&json).unwrap()).unwrap();
    let again = crate::read(&gltf).unwrap();
    assert_eq!(again.positions, back.positions);
    assert_eq!(again.triangles, back.triangles);
    std::fs::remove_dir_all(dir).ok();
}

/// The humanoid as a glTF of one node per shell, each shell moved off its
/// place and the node's translation moving it back.
#[test]
fn a_multi_mesh_gltf_with_node_transforms_fits() {
    let body = humanoid(Pose::T);
    let parts = body
        .shells
        .iter()
        .enumerate()
        .map(|(i, shell)| {
            let offset = Vec3::new(i as f32 * 0.1, 0.3, -0.2);
            let mut mesh = shell.clone();
            for p in mesh.positions.iter_mut() {
                *p -= offset;
            }
            crate::PartMesh {
                part: Part::ALL[i % 15],
                centre: offset,
                size: Vec3::ONE,
                mesh,
                attachments: Vec::new(),
                accessories: Vec::new(),
                cap_triangles: 0,
            }
        })
        .collect();
    let bytes = crate::gltf(&crate::Body {
        parts,
        warnings: Vec::new(),
    });
    let mesh = crate::read_bytes(&bytes, crate::Format::Gltf).unwrap();
    assert_eq!(mesh.triangles.len(), body.mesh.triangles.len());
    let fit = fit(&mesh, HEIGHT).unwrap();
    assert_eq!(fit.orientation, Orientation::ROBLOX);
    assert_joints(&fit, &body, 0.05);
}

#[test]
fn an_obj_reads_back_with_flipped_uvs() {
    let body = humanoid(Pose::A);
    let mesh = to_file(
        &body.mesh,
        Orientation::new(Axis::PosZ, Axis::NegY).unwrap(),
        1.,
    );
    let mut text = String::from("# synthetic\no body\n");
    for p in &mesh.positions {
        text += &format!("v {} {} {}\n", p.x, p.y, p.z);
    }
    for t in &mesh.uvs {
        text += &format!("vt {} {}\n", t.x, 1. - t.y);
    }
    for n in &mesh.normals {
        text += &format!("vn {} {} {}\n", n.x, n.y, n.z);
    }
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| i + 1);
        text += &format!("f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\n");
    }
    let dir = scratch("obj");
    let path = dir.join("body.OBJ");
    std::fs::write(&path, text).unwrap();
    let back = crate::read(&path).unwrap();
    std::fs::remove_dir_all(dir).ok();
    assert_eq!(back.triangles.len(), mesh.triangles.len());
    assert_eq!(back.uvs.len(), back.positions.len());
    assert_eq!(back.normals.len(), back.positions.len());
    // Every corner lands where it was, with its UV.
    for (t, u) in mesh.triangles.iter().zip(&back.triangles) {
        for k in 0..3 {
            let (i, j) = (t[k] as usize, u[k] as usize);
            assert!((mesh.positions[i] - back.positions[j]).length() < 1e-5);
            assert!((mesh.uvs[i] - back.uvs[j]).length() < 1e-5);
        }
    }
    let fit = fit(&back, HEIGHT).unwrap();
    assert_eq!(
        fit.orientation,
        Orientation::new(Axis::PosZ, Axis::NegY).unwrap()
    );
}

#[test]
fn a_binary_fbx_reads_geometry_uvs_and_transforms() {
    use fbxcel::low::FbxVersion;
    use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

    let mut out = Vec::new();
    let mut w = Writer::new(std::io::Cursor::new(&mut out), FbxVersion::V7_4).unwrap();
    w.new_node("Objects").unwrap();
    {
        let mut a = w.new_node("Geometry").unwrap();
        a.append_i64(10).unwrap();
        a.append_string_direct("Quad\0\u{1}Geometry").unwrap();
        a.append_string_direct("Mesh").unwrap();
    }
    {
        let mut a = w.new_node("Vertices").unwrap();
        a.append_arr_f64_from_iter(None, [0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.])
            .unwrap();
    }
    w.close_node().unwrap();
    {
        let mut a = w.new_node("PolygonVertexIndex").unwrap();
        a.append_arr_i32_from_iter(None, [0, 1, 2, !3]).unwrap();
    }
    w.close_node().unwrap();
    w.new_node("LayerElementUV").unwrap();
    for (name, value) in [
        ("MappingInformationType", "ByPolygonVertex"),
        ("ReferenceInformationType", "IndexToDirect"),
    ] {
        let mut a = w.new_node(name).unwrap();
        a.append_string_direct(value).unwrap();
        w.close_node().unwrap();
    }
    {
        let mut a = w.new_node("UV").unwrap();
        a.append_arr_f64_from_iter(None, [0., 0., 1., 0., 1., 1., 0., 1.])
            .unwrap();
    }
    w.close_node().unwrap();
    {
        let mut a = w.new_node("UVIndex").unwrap();
        a.append_arr_i32_from_iter(None, [0, 1, 2, 3]).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap(); // LayerElementUV
    w.close_node().unwrap(); // Geometry
    {
        let mut a = w.new_node("Model").unwrap();
        a.append_i64(20).unwrap();
        a.append_string_direct("Quad\0\u{1}Model").unwrap();
        a.append_string_direct("Mesh").unwrap();
    }
    w.new_node("Properties70").unwrap();
    {
        let mut a = w.new_node("P").unwrap();
        for s in ["Lcl Translation", "Lcl Translation", "", "A"] {
            a.append_string_direct(s).unwrap();
        }
        for v in [5., 0., 0.] {
            a.append_f64(v).unwrap();
        }
    }
    w.close_node().unwrap();
    w.close_node().unwrap(); // Properties70
    w.close_node().unwrap(); // Model
    w.close_node().unwrap(); // Objects
    w.new_node("Connections").unwrap();
    {
        let mut a = w.new_node("C").unwrap();
        a.append_string_direct("OO").unwrap();
        a.append_i64(10).unwrap();
        a.append_i64(20).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.finalize_and_flush(&FbxFooter::default()).unwrap();

    let mesh = crate::read_bytes(&out, crate::Format::Fbx).unwrap();
    assert_eq!(mesh.triangles.len(), 2);
    assert_eq!(mesh.positions.len(), 4);
    assert_eq!(mesh.positions[2], Vec3::new(6., 1., 0.));
    assert_eq!(mesh.uvs[2], glam::Vec2::new(1., 0.));
    assert_eq!(mesh.uvs[0], glam::Vec2::new(0., 1.));
    assert!(mesh.normals.is_empty());
    assert!(matches!(
        crate::read_bytes(b"; FBX 7.4.0 project file", crate::Format::Fbx),
        Err(crate::Error::Format(_))
    ));
}

#[test]
fn unknown_extensions_are_refused_by_name() {
    let err = crate::read(std::path::Path::new("body.stl")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "can\u{2019}t read .stl files: use .gltf, .glb, .obj or .fbx"
    );
}

/// Run with `cargo test --release -p rbx_autorig -- --ignored`: the target
/// is fit plus split of a 20k-triangle body in well under a second.
#[test]
#[ignore]
fn twenty_thousand_triangles_fit_and_split_in_under_a_second() {
    let body = humanoid::humanoid_with(Pose::A, 0.0108);
    let triangles = body.mesh.triangles.len();
    let start = std::time::Instant::now();
    let fit = fit(&body.mesh, HEIGHT).unwrap();
    let fitted = start.elapsed();
    let parts = split(&body.mesh, &fit).unwrap();
    let total = start.elapsed();
    eprintln!(
        "{triangles} triangles: fit {fitted:?}, split {:?}, total {total:?}, {} out",
        total - fitted,
        parts.triangle_count()
    );
    assert_joints(&fit, &body, 0.05);
    if !cfg!(debug_assertions) {
        assert!(total.as_secs_f32() < 1., "{total:?}");
    }
}

#[test]
fn every_part_carries_the_mannequins_accessory_attachments() {
    // The Mannequin's set, part by part (the Rig Builder's R15 tables).
    let want: [(Part, &[&str]); 15] = [
        (Part::Head, &["Hat", "Hair", "FaceFront", "FaceCenter"]),
        (
            Part::UpperTorso,
            &["Neck", "LeftCollar", "RightCollar", "BodyFront", "BodyBack"],
        ),
        (
            Part::LowerTorso,
            &["WaistCenter", "WaistFront", "WaistBack"],
        ),
        (Part::LeftUpperArm, &["LeftShoulder"]),
        (Part::LeftLowerArm, &[]),
        (Part::LeftHand, &["LeftGrip"]),
        (Part::RightUpperArm, &["RightShoulder"]),
        (Part::RightLowerArm, &[]),
        (Part::RightHand, &["RightGrip"]),
        (Part::LeftUpperLeg, &[]),
        (Part::LeftLowerLeg, &[]),
        (Part::LeftFoot, &["LeftFoot"]),
        (Part::RightUpperLeg, &[]),
        (Part::RightLowerLeg, &[]),
        (Part::RightFoot, &["RightFoot"]),
    ];
    for pose in [Pose::A, Pose::T] {
        let (_, fit, body) = check_split(pose);
        let world = |part: Part, name: &str| {
            let piece = body.part(part);
            let (_, at) = piece
                .accessories
                .iter()
                .find(|(n, _)| n == &format!("{name}Attachment"))
                .unwrap_or_else(|| panic!("{part} has no {name}Attachment"));
            piece.centre + *at
        };
        for (part, names) in want {
            let mut got: Vec<&str> = body
                .part(part)
                .accessories
                .iter()
                .map(|(n, _)| n.strip_suffix("Attachment").unwrap())
                .collect();
            got.sort();
            let mut names = names.to_vec();
            names.sort();
            assert_eq!(got, names, "{pose:?} {part}");
            // Each sits on or in its part's bounds.
            let piece = body.part(part);
            for (name, at) in &piece.accessories {
                let slack = Vec3::splat(1e-4);
                assert!(
                    at.abs().cmple(piece.size / 2. + slack).all(),
                    "{pose:?} {name} at {at:?} outside {:?}",
                    piece.size
                );
            }
        }
        // Where they matter: the hat on the crown, the grips in the hands,
        // the feet on the ground, the front of the body in front (−Z).
        let hat = world(Part::Head, "Hat");
        assert!(
            hat.y > fit.extremities.head.y - 0.02 * HEIGHT,
            "{pose:?} hat {hat:?}"
        );
        for (part, side, name) in [
            (Part::LeftHand, Side::Left, "LeftGrip"),
            (Part::RightHand, Side::Right, "RightGrip"),
        ] {
            let grip = world(part, name);
            let wrist = fit.joint(part.joint());
            let tip = fit.extremities.hand(side);
            assert!(
                (grip - wrist.lerp(tip, 0.5)).length() < 1e-4,
                "{pose:?} {name}"
            );
        }
        for (part, name) in [(Part::LeftFoot, "LeftFoot"), (Part::RightFoot, "RightFoot")] {
            assert!(world(part, name).y < 0.01 * HEIGHT, "{pose:?} {name}");
        }
        assert!(world(Part::UpperTorso, "BodyFront").z < world(Part::UpperTorso, "BodyBack").z);
        assert!(world(Part::LowerTorso, "WaistFront").z < world(Part::LowerTorso, "WaistBack").z);
        let neck = world(Part::UpperTorso, "Neck");
        assert!((neck - fit.joint(Joint::Neck)).length() < 1e-4);
        let (l, r) = (
            world(Part::UpperTorso, "LeftCollar"),
            world(Part::UpperTorso, "RightCollar"),
        );
        assert!(l.x < neck.x && neck.x < r.x && l.y < neck.y);
    }
}
