//! Skin weights, the skinned `version 4.00` mesh and the skinned `.glb`.

use std::sync::OnceLock;

use glam::{Affine3A, Mat4, Quat, Vec3};

use super::humanoid::{Humanoid, Pose};
use super::{check_split, HEIGHT};
use crate::{Body, Fit, Joint, Part, PartMesh, PartSkin, Skin};

/// The pose's split with its weights, computed once.
fn skinned(
    pose: Pose,
) -> (
    &'static Humanoid,
    &'static Fit,
    &'static Body,
    &'static Skin,
) {
    static SKINS: [OnceLock<Skin>; 5] = [const { OnceLock::new() }; 5];
    let (human, fit, body) = check_split(pose);
    let skin = SKINS[pose as usize].get_or_init(|| crate::weights(&human.mesh, fit, body));
    (human, fit, body, skin)
}

#[test]
fn every_vertex_has_at_most_four_influences_summing_to_255() {
    for pose in [Pose::A, Pose::T, Pose::I] {
        let (_, fit, body, skin) = skinned(pose);
        assert_eq!(skin.parts.len(), 15);
        assert_eq!(skin.joints, fit.joints);
        for (piece, ps) in body.parts.iter().zip(&skin.parts) {
            assert_eq!(ps.part, piece.part);
            assert_eq!(ps.bones, crate::bones_of(piece.part));
            assert_eq!(ps.influences.len(), piece.mesh.positions.len());
            // The own bone's bind is the part's own rig attachment.
            assert_eq!(piece.attachments[0].0, piece.part.joint().attachment());
            assert!((ps.binds[ps.own()] - piece.attachments[0].1).length() < 1e-5);
            for (bone, bind) in ps.bones.iter().zip(&ps.binds) {
                let world = piece.centre + *bind;
                assert!((world - fit.joint(bone.joint())).length() < 1e-4);
            }
            for inf in &ps.influences {
                assert_eq!(inf.iter().map(|e| u32::from(e.1)).sum::<u32>(), 255);
                assert!(inf.iter().all(|e| (e.0 as usize) < ps.bones.len()));
                assert!(inf.windows(2).all(|p| p[0].1 >= p[1].1), "{inf:?}");
                // No bone twice with weight.
                let live: Vec<u8> = inf.iter().filter(|e| e.1 > 0).map(|e| e.0).collect();
                let mut unique = live.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(unique.len(), live.len(), "{inf:?}");
            }
        }
    }
}

/// Where along `a`→`b` the point `q` projects, 0 at `a`, 1 at `b`.
fn along(q: Vec3, a: Vec3, b: Vec3) -> f32 {
    (q - a).dot(b - a) / (b - a).length_squared()
}

/// Cap vertices come after every wall vertex.
fn first_cap_vertex(piece: &PartMesh) -> usize {
    let tris = &piece.mesh.triangles;
    tris[tris.len() - piece.cap_triangles..]
        .iter()
        .flatten()
        .map(|&i| i as usize)
        .min()
        .unwrap_or(piece.mesh.positions.len())
}

#[test]
fn vertices_deep_inside_a_part_follow_its_own_bone() {
    for pose in [Pose::A, Pose::T, Pose::I] {
        let (_, fit, body, skin) = skinned(pose);
        let j = |joint: Joint| fit.joint(joint);
        let e = &fit.extremities;
        let segs = crate::split::segments(fit);
        let mut report = String::new();
        for part in Part::ALL {
            let piece = body.part(part);
            let ps = skin.part(part);
            let start = j(part.joint());
            // The bone's far end: the child joint, or the extremity.
            let end = match part {
                Part::Head => e.head,
                Part::LeftHand => e.left_hand,
                Part::RightHand => e.right_hand,
                Part::LeftFoot => e.left_foot,
                Part::RightFoot => e.right_foot,
                Part::UpperTorso => j(Joint::Neck),
                Part::LowerTorso => continue,
                _ => j(Joint::ALL
                    .into_iter()
                    .find(|c| c.parent_part() == Some(part))
                    .unwrap()),
            };
            let (from, to) = match part {
                Part::Head | Part::LeftHand | Part::RightHand => (0.6, 2.),
                Part::LeftFoot | Part::RightFoot => (0.7, 2.),
                Part::UpperTorso => (0.45, 0.7),
                _ => (0.35, 0.65),
            };
            let mut worst = 255u8;
            let mut count = 0;
            let mut at = String::new();
            // Caps are inside the body, not flesh.
            let walls = first_cap_vertex(piece);
            for (i, v) in piece.mesh.positions.iter().enumerate().take(walls) {
                let q = piece.centre + *v;
                let t = along(q, start, end);
                if t < from || t > to {
                    continue;
                }
                // Deep also means clearly nearest its own bone: not torso
                // flank the shoulder cut handed to an arm, say.
                let nearest = |own: bool| {
                    segs.iter()
                        .filter(|s| (s.0 == part) == own)
                        .map(|s| crate::fit::segment_distance(q, s.1, s.2))
                        .fold(f32::INFINITY, f32::min)
                };
                if nearest(true) > 0.6 * nearest(false) {
                    continue;
                }
                // And well away from every joint of the part.
                let near_joint = Joint::ALL.into_iter().any(|joint| {
                    (joint.part() == part || joint.parent_part() == Some(part))
                        && (q - j(joint)).length() < 0.06 * HEIGHT
                });
                if near_joint {
                    continue;
                }
                count += 1;
                if ps.weight(i, part) < worst {
                    worst = ps.weight(i, part);
                    at = format!(
                        "{:?} t {t:.2} {:?}",
                        q.to_array(),
                        ps.weights_of(i).collect::<Vec<_>>()
                    );
                }
            }
            report += &format!(
                "{pose:?} {part}: {count} deep vertices, least own weight {worst} at {at}\n"
            );
            assert!(count > 0, "{report}");
            assert!(worst >= 230, "{report}");
        }
        eprint!("{report}");
    }
}

/// How many of `limb`'s wall vertices lie between 0.35 and 0.8 of the way
/// down its bone (and 0.06·H clear of both joints), and the most weight any
/// of them gives `other`.
fn bleed(pose: Pose, limb: Part, other: Part) -> (usize, u8) {
    let (_, fit, body, skin) = skinned(pose);
    let piece = body.part(limb);
    let ps = skin.part(limb);
    let start = fit.joint(limb.joint());
    let end = fit.joint(
        Joint::ALL
            .into_iter()
            .find(|c| c.parent_part() == Some(limb))
            .unwrap(),
    );
    let (mut count, mut most) = (0, 0);
    for (i, v) in piece
        .mesh
        .positions
        .iter()
        .enumerate()
        .take(first_cap_vertex(piece))
    {
        let q = piece.centre + *v;
        let t = along(q, start, end);
        let clear = (q - start).length() > 0.06 * HEIGHT && (q - end).length() > 0.06 * HEIGHT;
        // Flesh of the limb, not torso flank the cut handed to it.
        let segs = crate::split::segments(fit);
        let nearest = |own: bool| {
            segs.iter()
                .filter(|s| (s.0 == limb) == own)
                .map(|s| crate::fit::segment_distance(q, s.1, s.2))
                .fold(f32::INFINITY, f32::min)
        };
        if (0.35..=0.8).contains(&t) && clear && nearest(true) <= 0.6 * nearest(false) {
            count += 1;
            most = most.max(ps.weight(i, other));
        }
    }
    (count, most)
}

#[test]
fn a_limb_pressed_against_the_body_keeps_none_of_its_weight() {
    // Before the parent's reach was capped along the child bone, an I-pose
    // upper arm kept up to 22/255 of UpperTorso 0.43 of the way to the
    // elbow, an A-pose one 13/255, the (touching) thighs up to 16/255 of
    // LowerTorso, and the pressed arm 3/255.
    let mut report = String::new();
    let mut worst = 0;
    for pose in [Pose::Pressed, Pose::I, Pose::A, Pose::T] {
        for (limb, other) in [
            (Part::LeftUpperArm, Part::UpperTorso),
            (Part::RightUpperArm, Part::UpperTorso),
            (Part::LeftUpperLeg, Part::LowerTorso),
            (Part::RightUpperLeg, Part::LowerTorso),
        ] {
            let (count, most) = bleed(pose, limb, other);
            report += &format!("{pose:?} {limb}: {count} vertices, {other} up to {most}/255\n");
            assert!(count > 0, "{report}");
            worst = worst.max(most);
        }
    }
    eprint!("{report}");
    assert!(worst <= 2, "{report}");
}

#[test]
fn vertices_on_a_joint_plane_are_shared_by_both_bones() {
    for pose in [Pose::A, Pose::T] {
        let (_, fit, body, skin) = skinned(pose);
        for joint in [
            Joint::LeftElbow,
            Joint::RightElbow,
            Joint::LeftKnee,
            Joint::RightKnee,
            Joint::LeftWrist,
        ] {
            let (point, normal) = crate::split::plane(fit, joint);
            let (child, parent) = (joint.part(), joint.parent_part().unwrap());
            let mut count = 0;
            for part in [child, parent] {
                let piece = body.part(part);
                let ps = skin.part(part);
                for (i, v) in piece.mesh.positions.iter().enumerate() {
                    let q = piece.centre + *v;
                    if (q - point).dot(normal).abs() > 1e-4 * HEIGHT {
                        continue;
                    }
                    count += 1;
                    let (c, p) = (ps.weight(i, child), ps.weight(i, parent));
                    assert!(
                        c < 242 && p < 242 && u32::from(c) + u32::from(p) >= 240,
                        "{pose:?} {joint} in {part}: vertex {i} has {child} {c}, {parent} {p}"
                    );
                }
            }
            assert!(
                count > 10,
                "{pose:?} {joint}: {count} vertices on the plane"
            );
        }
    }
}

/// World positions of `piece`'s vertices with every part at `cframe(part)`,
/// by the skinning convention: `Mᵢ = (part.CFrame⁻¹ · boneJointWorldNowᵢ) ·
/// bindᵢ⁻¹`, `boneJointWorldNowᵢ = bone part's CFrame · its own rig
/// attachment`.
fn deform(
    piece: &PartMesh,
    ps: &PartSkin,
    body: &Body,
    cframe: &dyn Fn(Part) -> Affine3A,
) -> Vec<Vec3> {
    let this = cframe(piece.part);
    let m: Vec<Affine3A> = ps
        .bones
        .iter()
        .zip(&ps.binds)
        .map(|(&bone, &bind)| {
            let attachment = body.part(bone).attachments[0].1;
            let joint_now = cframe(bone) * Affine3A::from_translation(attachment);
            this.inverse() * joint_now * Affine3A::from_translation(bind).inverse()
        })
        .collect();
    piece
        .mesh
        .positions
        .iter()
        .zip(&ps.influences)
        .map(|(&v, inf)| {
            let local: Vec3 = inf
                .iter()
                .map(|&(slot, w)| m[slot as usize].transform_point3(v) * (f32::from(w) / 255.))
                .sum();
            this.transform_point3(local)
        })
        .collect()
}

#[test]
fn bending_the_elbow_bends_the_forearm_smoothly() {
    let (_, fit, body, skin) = skinned(Pose::A);
    let elbow = fit.joint(Joint::LeftElbow);
    let (shoulder, wrist) = (fit.joint(Joint::LeftShoulder), fit.joint(Joint::LeftWrist));
    let turn = Affine3A::from_translation(elbow)
        * Affine3A::from_quat(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
        * Affine3A::from_translation(-elbow);
    let rest = |part: Part| Affine3A::from_translation(body.part(part).centre);
    let bent = |part: Part| match part {
        Part::LeftLowerArm | Part::LeftHand => turn * rest(part),
        _ => rest(part),
    };
    // At rest nothing moves.
    for part in Part::ALL {
        let piece = body.part(part);
        let moved = deform(piece, skin.part(part), body, &rest);
        for (v, q) in piece.mesh.positions.iter().zip(&moved) {
            assert!(
                (piece.centre + *v - *q).length() < 1e-4,
                "{part} moves at rest"
            );
        }
    }

    let mut deformed = std::collections::HashMap::new();
    for part in [Part::LeftUpperArm, Part::LeftLowerArm, Part::LeftHand] {
        let piece = body.part(part);
        let moved = deform(piece, skin.part(part), body, &bent);
        let mut forearm_error = 0f32;
        let mut shoulder_drift = 0f32;
        let mut upper_drift = 0f32;
        let mut stretch = 0f32;
        for (i, v) in piece.mesh.positions.iter().enumerate() {
            let q = piece.centre + *v;
            let d = moved[i];
            // No vertex travels farther than a rigid quarter turn about the
            // elbow would take it.
            let bound = std::f32::consts::SQRT_2 * (q - elbow).length() * 1.01 + 1e-3;
            assert!((d - q).length() <= bound, "{part} vertex {i} jumps");
            match part {
                Part::LeftHand => {
                    forearm_error = forearm_error.max((d - turn.transform_point3(q)).length())
                }
                Part::LeftLowerArm if along(q, elbow, wrist) > 0.5 => {
                    forearm_error = forearm_error.max((d - turn.transform_point3(q)).length());
                }
                Part::LeftUpperArm if along(q, shoulder, elbow) < 0.2 => {
                    shoulder_drift = shoulder_drift.max((d - q).length());
                }
                Part::LeftUpperArm if along(q, shoulder, elbow) < 0.4 => {
                    upper_drift = upper_drift.max((d - q).length());
                }
                _ => {}
            }
        }
        // Smoothness: no edge stretches or shrinks wildly.
        for t in &piece.mesh.triangles {
            for k in 0..3 {
                let (a, b) = (t[k] as usize, t[(k + 1) % 3] as usize);
                let before = (piece.mesh.positions[a] - piece.mesh.positions[b]).length();
                let after = (moved[a] - moved[b]).length();
                if before > 1e-3 * HEIGHT {
                    stretch = stretch.max(after / before);
                }
            }
        }
        eprintln!(
            "{part}: forearm error {forearm_error:.4}, shoulder drift {shoulder_drift:.5}, upper arm drift {upper_drift:.4}, worst edge stretch {stretch:.2}"
        );
        assert!(forearm_error < 0.02 * HEIGHT, "{part}: {forearm_error}");
        // Next to the shoulder nothing moves; up to 40% of the way down,
        // the elbow's heat is a few percent at most.
        assert!(shoulder_drift < 1e-3, "{part}: {shoulder_drift}");
        assert!(upper_drift < 0.005 * HEIGHT, "{part}: {upper_drift}");
        assert!(stretch < 3., "{part}: {stretch}");
        deformed.insert(part, moved);
    }
    // The forearm really turned.
    let lower = body.part(Part::LeftLowerArm);
    let tip = lower
        .mesh
        .positions
        .iter()
        .enumerate()
        .max_by(|a, b| {
            along(lower.centre + *a.1, elbow, wrist).total_cmp(&along(
                lower.centre + *b.1,
                elbow,
                wrist,
            ))
        })
        .unwrap()
        .0;
    assert!(
        (deformed[&Part::LeftLowerArm][tip] - (lower.centre + lower.mesh.positions[tip])).length()
            > 0.1 * HEIGHT
    );

    // The seam between upper and lower arm stays closed.
    let upper = body.part(Part::LeftUpperArm);
    let mut seam = 0;
    let mut gap = 0f32;
    for (i, v) in upper.mesh.positions.iter().enumerate() {
        let q = upper.centre + *v;
        for (k, w) in lower.mesh.positions.iter().enumerate() {
            if (lower.centre + *w - q).length() < 1e-5 * HEIGHT {
                seam += 1;
                gap = gap.max(
                    (deformed[&Part::LeftUpperArm][i] - deformed[&Part::LeftLowerArm][k]).length(),
                );
            }
        }
    }
    eprintln!("{seam} seam vertex pairs, widest gap {gap:.4}");
    assert!(seam > 10);
    assert!(gap < 0.005 * HEIGHT, "{gap}");
}

#[test]
fn mesh_v4_round_trips_geometry_and_skin() {
    let (_, _, body, skin) = skinned(Pose::A);
    for part in [Part::UpperTorso, Part::LeftLowerArm, Part::LowerTorso] {
        let piece = &body.part(part).mesh;
        let ps = skin.part(part);
        let bytes = crate::mesh_v4(piece, Some(ps));
        let parsed = rbx_mesh::parse(&bytes).unwrap();
        assert_eq!(parsed.version, (4, 0));
        let (nv, nf) = (piece.positions.len(), piece.triangles.len());
        assert_eq!(parsed.vertices.len(), nv);
        for (i, v) in parsed.vertices.iter().enumerate() {
            assert_eq!(v.position, piece.positions[i].to_array());
            assert_eq!(v.normal, piece.normals[i].to_array());
            assert_eq!(v.uv, piece.uvs[i].to_array());
            assert_eq!(v.color, [255; 4]);
        }
        let indices: Vec<u32> = piece.triangles.iter().flatten().copied().collect();
        assert_eq!(parsed.indices, indices);
        assert_eq!(parsed.lods, vec![0..nf as u32]);

        // The skinning blocks, decoded by hand from the layout.
        let u16_at = |o: usize| u16::from_le_bytes(bytes[o..o + 2].try_into().unwrap());
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let h = 13;
        let nb = ps.bones.len();
        assert_eq!(u16_at(h), 24);
        assert_eq!(u32_at(h + 4) as usize, nv);
        assert_eq!(u32_at(h + 8) as usize, nf);
        assert_eq!(u16_at(h + 12), 2);
        assert_eq!(u16_at(h + 14) as usize, nb);
        let names_len = u32_at(h + 16) as usize;
        assert_eq!(u16_at(h + 20), 1);
        let envelopes = h + 24 + nv * 40;
        for (i, inf) in ps.influences.iter().enumerate() {
            let e = &bytes[envelopes + 8 * i..envelopes + 8 * i + 8];
            assert_eq!(e[..4], inf.map(|x| x.0));
            assert_eq!(e[4..], inf.map(|x| x.1));
            assert_eq!(e[4..].iter().map(|&w| u32::from(w)).sum::<u32>(), 255);
        }
        let lods = envelopes + nv * 8 + nf * 12;
        assert_eq!((u32_at(lods), u32_at(lods + 4)), (0, nf as u32));
        let bones = lods + 8;
        let names = bones + nb * 60;
        for (k, bone) in ps.bones.iter().enumerate() {
            let b = bones + 60 * k;
            let name_at = names + u32_at(b) as usize;
            let end = bytes[name_at..].iter().position(|&c| c == 0).unwrap();
            assert_eq!(&bytes[name_at..name_at + end], bone.name().as_bytes());
            let parent = u16_at(b + 4);
            let want = bone
                .parent()
                .and_then(|p| ps.bones.iter().position(|&x| x == p))
                .map_or(0xFFFF, |i| i as u16);
            assert_eq!(parent, want, "{bone}");
            assert!(parent == 0xFFFF || (parent as usize) < k, "parents first");
            assert_eq!(u16_at(b + 6), parent);
            assert_eq!(f32_at(b + 8), 0.);
            let m: Vec<f32> = (0..12).map(|i| f32_at(b + 12 + 4 * i)).collect();
            assert_eq!(m[..9], [1., 0., 0., 0., 1., 0., 0., 0., 1.]);
            assert_eq!(m[9..], ps.binds[k].to_array());
        }
        let subset = names + names_len;
        assert_eq!(
            (0..5).map(|i| u32_at(subset + 4 * i)).collect::<Vec<_>>(),
            [0, nf as u32, 0, nv as u32, nb as u32]
        );
        for k in 0..26 {
            let want = if k < nb { k as u16 } else { 0xFFFF };
            assert_eq!(u16_at(subset + 20 + 2 * k), want);
        }
        assert_eq!(bytes.len(), subset + 72);
    }

    // Unskinned, it is plain geometry.
    let piece = &body.part(Part::Head).mesh;
    let bytes = crate::mesh_v4(piece, None);
    let parsed = rbx_mesh::parse(&bytes).unwrap();
    assert_eq!(parsed.vertices.len(), piece.positions.len());
    assert_eq!(
        bytes.len(),
        13 + 24 + piece.positions.len() * 40 + piece.triangles.len() * 12 + 8
    );
}

#[test]
fn the_skinned_glb_has_one_skin_over_the_r15_armature() {
    let (_, _, body, skin) = skinned(Pose::A);
    let bytes = crate::gltf_skinned(body, skin);
    let file = gltf::Gltf::from_slice(&bytes).unwrap();
    let blob = file.blob.as_deref().unwrap();
    let root = skin.joints[Joint::Root as usize];

    assert_eq!(file.skins().count(), 1);
    let gskin = file.skins().next().unwrap();
    let joints: Vec<gltf::Node> = gskin.joints().collect();
    let names: Vec<&str> = joints.iter().map(|n| n.name().unwrap()).collect();
    let mut want = vec!["Root", "HumanoidRootNode"];
    want.extend(Part::ALL.iter().map(|p| p.name()));
    assert_eq!(names, want);
    assert_eq!(gskin.skeleton().unwrap().name(), Some("Root"));

    // World transforms of the joints, walking down from the scene.
    let mut world = vec![Mat4::IDENTITY; file.nodes().count()];
    fn walk(node: gltf::Node, parent: Mat4, world: &mut [Mat4]) {
        let m = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        world[node.index()] = m;
        for child in node.children() {
            walk(child, m, world);
        }
    }
    let scene = file.scenes().next().unwrap();
    for node in scene.nodes() {
        walk(node, Mat4::IDENTITY, &mut world);
    }
    // Hierarchy: Root → HumanoidRootNode → LowerTorso → … as R15 parts.
    for node in &joints[2..] {
        let part = Part::ALL[node.index() - 2];
        let parent = file
            .nodes()
            .find(|n| n.children().any(|c| c.index() == node.index()))
            .unwrap();
        let want = part.parent().map_or("HumanoidRootNode", Part::name);
        assert_eq!(parent.name(), Some(want));
        let (_, rotation, _) = node.transform().decomposed();
        assert_eq!(rotation, [0., 0., 0., 1.]);
        // At its joint, measured from the Root joint.
        let at = world[node.index()].w_axis.truncate();
        assert!(
            (at - (skin.bone_position(part) - root)).length() < 1e-4,
            "{part}"
        );
    }
    for name in ["Root", "HumanoidRootNode", "LowerTorso"] {
        let node = joints.iter().find(|n| n.name() == Some(name)).unwrap();
        assert!(
            world[node.index()].w_axis.truncate().length() < 1e-6,
            "{name}"
        );
    }
    let inverse: Vec<Mat4> = gskin
        .reader(|_| Some(blob))
        .read_inverse_bind_matrices()
        .unwrap()
        .map(|m| Mat4::from_cols_array_2d(&m))
        .collect();
    assert_eq!(inverse.len(), joints.len());
    for (node, ibm) in joints.iter().zip(&inverse) {
        assert!((world[node.index()] * *ibm).abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }

    let mut meshes = 0;
    for node in file.nodes().filter(|n| n.mesh().is_some()) {
        meshes += 1;
        assert!(node.skin().is_some());
        let name = node.name().unwrap();
        let part = Part::ALL
            .into_iter()
            .find(|p| format!("{}_Geo", p.name()) == name)
            .unwrap();
        assert_eq!(node.mesh().unwrap().name(), Some(name));
        let piece = body.part(part);
        let primitive = node.mesh().unwrap().primitives().next().unwrap();
        let reader = primitive.reader(|_| Some(blob));
        let positions: Vec<Vec3> = reader.read_positions().unwrap().map(Vec3::from).collect();
        let joints_of: Vec<[u16; 4]> = reader.read_joints(0).unwrap().into_u16().collect();
        let weights: Vec<[f32; 4]> = reader.read_weights(0).unwrap().into_f32().collect();
        assert_eq!(positions.len(), piece.mesh.positions.len());
        for i in 0..positions.len() {
            assert!((positions[i] + root - piece.centre - piece.mesh.positions[i]).length() < 1e-4);
            assert!((weights[i].iter().sum::<f32>() - 1.).abs() < 1e-5);
            let mut skinned = Vec3::ZERO;
            for k in 0..4 {
                let j = joints_of[i][k] as usize;
                if weights[i][k] > 0. {
                    assert!(j >= 2, "{name}: weight on {}", want[j]);
                }
                skinned += (world[joints[j].index()] * inverse[j]).transform_point3(positions[i])
                    * weights[i][k];
            }
            // At rest, skinning leaves every vertex where it is.
            assert!((skinned - positions[i]).length() < 1e-4);
        }
    }
    assert_eq!(meshes, 15);
}

/// Run with `cargo test --release -p rbx_autorig -- --ignored`: weights for
/// a 20k-triangle body in well under two seconds.
#[test]
#[ignore]
fn twenty_thousand_triangles_skin_in_under_two_seconds() {
    let human = super::humanoid::humanoid_with(Pose::A, 0.0108);
    let fit = crate::fit(&human.mesh, HEIGHT).unwrap();
    let body = crate::split(&human.mesh, &fit).unwrap();
    let start = std::time::Instant::now();
    let skin = crate::weights(&human.mesh, &fit, &body);
    let took = start.elapsed();
    eprintln!(
        "{} triangles in, {} out: weights in {took:?}",
        human.mesh.triangles.len(),
        body.triangle_count()
    );
    assert_eq!(skin.parts.len(), 15);
    if !cfg!(debug_assertions) {
        assert!(took.as_secs_f32() < 2., "{took:?}");
    }
}
