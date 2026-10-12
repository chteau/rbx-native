//! Shapes real downloaded characters showed the fit getting wrong: a shadow
//! disc and a prop on the floor (TIBY), a head bigger than the torso whose
//! hair outreaches the hands (TIBY, Rak), and a glTF that requires a
//! material extension (Kenney's blocky characters).

use glam::Vec3;

use super::humanoid::{aabb, capsule, humanoid, sphere, Pose};
use super::HEIGHT;
use crate::{fit, Joint, Mesh};

#[test]
fn a_ground_disc_and_a_prop_on_the_floor_are_left_out() {
    let body = humanoid(Pose::A);
    let mut mesh = body.mesh.clone();
    let clean = mesh.triangles.len();
    // A flat square the feet stand on, and a ball off to one side.
    let disc = aabb(Vec3::new(-0.3, 0., -0.3), Vec3::new(0.3, 0.0004, 0.3), 0.05);
    let ball = sphere(Vec3::new(0.5, 0.05, 0.), 0.05);
    let (disc_n, ball_n) = (disc.triangles.len(), ball.triangles.len());
    mesh.append(disc);
    mesh.append(ball);

    let dropped = crate::mesh::drop_debris(&mut mesh);
    assert_eq!(dropped.bases, disc_n);
    assert_eq!(dropped.detached, ball_n);
    assert_eq!(mesh.triangles.len(), clean);
    assert_eq!(mesh.notes.len(), 2);

    let fit = fit(&mesh, HEIGHT).unwrap();
    super::assert_joints(&fit, &body, 0.05);
    assert!(fit.warnings.iter().any(|w| w.contains("flat base")));
}

#[test]
fn the_body_itself_is_never_left_out() {
    // Separate shells that touch (head, neck, limbs) all stay.
    let body = humanoid(Pose::T);
    let mut mesh = body.mesh.clone();
    let dropped = crate::mesh::drop_debris(&mut mesh);
    assert_eq!(dropped, crate::mesh::Dropped::default());
    assert_eq!(mesh.triangles.len(), body.mesh.triangles.len());
}

/// The A-pose humanoid with its head swapped for one 0.4 tall and wide on a
/// longer neck, as a chibi's: the thickest voxel is then in the head.
fn chibi() -> (Mesh, [Vec3; 15]) {
    let body = humanoid(Pose::A);
    let mut mesh = Mesh::default();
    // Shell 0 is the head sphere, shell 1 the neck.
    mesh.append(sphere(Vec3::new(0., 1.1, 0.), 0.2));
    mesh.append(capsule(
        Vec3::new(0., 0.80, 0.),
        Vec3::new(0., 0.95, 0.),
        0.03,
        0.03,
        0.025,
    ));
    for shell in &body.shells[2..] {
        mesh.append(shell.clone());
    }
    (mesh, body.joints)
}

#[test]
fn a_big_headed_chibi_keeps_its_head_hands_and_feet() {
    let (mesh, joints) = chibi();
    let fit = fit(&mesh, HEIGHT).unwrap();
    let s = HEIGHT / 1.3;
    let e = &fit.extremities;
    // The head tip is the top of the head, not a foot or a hair spike.
    assert!(e.head.y > 0.95 * HEIGHT, "head {:?}", e.head);
    assert!(e.left_hand.x < -0.2 * s && e.right_hand.x > 0.2 * s);
    assert!(e.left_foot.y < 0.05 * HEIGHT && e.right_foot.y < 0.05 * HEIGHT);
    assert!(e.left_foot.x < 0. && e.right_foot.x > 0.);
    // The neck between the torso and the head, found rather than guessed.
    let neck = fit.joint(Joint::Neck).y / s;
    assert!((0.82..=0.92).contains(&neck), "neck at {neck}");
    assert!(!fit.warnings.iter().any(|w| w.contains("neck")));
    for joint in [
        Joint::LeftHip,
        Joint::LeftKnee,
        Joint::LeftElbow,
        Joint::RightWrist,
    ] {
        let got = fit.joint(joint) / s;
        let want = joints[joint as usize];
        assert!(
            (got - want).length() < 0.06,
            "{}: got {got:?} want {want:?}",
            joint.name()
        );
    }
}

#[test]
fn a_gltf_requiring_a_material_extension_reads() {
    let (_, _, body) = super::check_split(Pose::A);
    let bytes = crate::gltf(body, &[]);
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    json["extensionsUsed"] = serde_json::json!(["KHR_materials_unlit", "KHR_texture_transform"]);
    json["extensionsRequired"] = serde_json::json!(["KHR_materials_unlit"]);
    let mut text = serde_json::to_vec(&json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let bin = &bytes[20 + json_len..];
    let mut glb = Vec::new();
    glb.extend_from_slice(&bytes[..8]);
    glb.extend_from_slice(&((12 + 8 + text.len() + bin.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(text.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&text);
    glb.extend_from_slice(bin);
    let mesh = crate::read_bytes(&glb, crate::Format::Gltf).unwrap();
    assert_eq!(mesh.triangles.len(), body.triangle_count());
}
