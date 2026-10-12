use std::f32::consts::FRAC_PI_2;

use glam::{Quat, Vec3};

use super::*;
use crate::fixture::Fixture;
use crate::rig::JointKind;
use crate::rig::Rig;

/// The fixture rig as a Blender export would hold it: Z-up, at half scale,
/// every bone turned its own way.
fn exported(raise: Quat) -> Skeleton {
    let to_file = Quat::from_rotation_x(FRAC_PI_2);
    let scale = 0.5;
    let file = |p: Vec3| to_file * p * scale;
    let worlds = [
        (
            "HumanoidRootPart",
            None,
            file(Vec3::new(0., 3., 0.)),
            Quat::from_rotation_z(0.3),
        ),
        (
            "Torso",
            Some(0),
            file(Vec3::new(0., 3., 0.)),
            Quat::from_rotation_y(-0.7),
        ),
        (
            "Right Arm",
            Some(1),
            file(Vec3::new(1., 3.5, 0.)),
            Quat::from_rotation_x(1.1),
        ),
        (
            "Head",
            Some(1),
            file(Vec3::new(0., 4., 0.)),
            Quat::from_rotation_y(2.),
        ),
    ];
    let mut bones: Vec<Bone> = Vec::new();
    for (name, parent, position, rotation) in worlds {
        let world = Frame::new(position, rotation);
        let parent_world = parent.map_or(Frame::IDENTITY, |p: usize| {
            Frame::new(worlds[p].2, worlds[p].3)
        });
        bones.push(Bone {
            name: name.into(),
            parent,
            rest: parent_world.inverse().mul(&world),
            scale: 1.,
            keys: Vec::new(),
        });
    }
    // Raise the arm about the shoulder: a world turn, carried into the file's
    // axes, applied to the bone's rest orientation.
    let turn = to_file * raise * to_file.conjugate();
    let arm_world = Frame::new(worlds[2].2, turn * worlds[2].3);
    let torso_world = Frame::new(worlds[1].2, worlds[1].3);
    let posed = torso_world.inverse().mul(&arm_world);
    bones[2].keys = vec![(0., bones[2].rest), (1., posed)];
    Skeleton { bones }
}

#[test]
fn fit_recovers_rotation_and_scale() {
    let q = Quat::from_rotation_x(FRAC_PI_2) * Quat::from_rotation_y(0.4);
    let from = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1., 2., 3.)];
    let to: Vec<Vec3> = from.iter().map(|p| q * *p * 2. + Vec3::splat(5.)).collect();
    let fit = fit(&from, &to).unwrap();
    assert!(fit.rotation.angle_between(q) < 1e-3, "{fit:?}");
    assert!((fit.scale - 2.).abs() < 1e-4);
    assert!(super::fit(&from[..2], &to[..2]).is_none());
}

#[test]
fn retargets_a_turned_scaled_z_up_skeleton() {
    let mut f = Fixture::new(JointKind::Motor6D);
    // A neck, so the matched pivots are not all on one line.
    let head = f.dom.new_instance("Part", "Head", Some(f.model));
    let neck = f.dom.new_instance("Motor6D", "Neck", Some(head));
    f.dom
        .set_property(neck, "Part0", rbx_dom::Variant::Ref(f.torso))
        .unwrap();
    f.dom
        .set_property(neck, "Part1", rbx_dom::Variant::Ref(head))
        .unwrap();
    let c0 = Frame::at(Vec3::new(0., 1., 0.)).to_data();
    f.dom
        .set_property(neck, "C0", rbx_dom::Variant::CFrame(c0))
        .unwrap();
    let rig = Rig::read(&f.dom, f.model);
    let raise = Quat::from_rotation_z(FRAC_PI_2);
    let clip = retarget(
        &exported(raise),
        &rig,
        Frame::at(Vec3::new(0., 3., 0.)),
        "Raise",
    );
    assert_eq!(clip.joints(), ["Right Arm"], "only keyed, matched bones");
    let start = clip.pose(0., "Right Arm").unwrap().cframe;
    assert!(start.approx_eq(&Frame::IDENTITY, 1e-3), "{start:?}");
    let end = clip.pose(1., "Right Arm").unwrap().cframe;
    // The shoulder's frame is unturned, so the Transform is the world raise.
    assert!(
        end.approx_eq(&Frame::new(Vec3::ZERO, raise), 1e-3),
        "{end:?}"
    );
}

#[test]
fn optimize_drops_keys_on_the_line_and_keeps_corners() {
    let mut clip = Clip::default();
    for (i, x) in [0., 1., 2., 3., 1.].iter().enumerate() {
        clip.set_pose(
            i as f32,
            "Head",
            PoseKey::new(Frame::at(Vec3::new(*x, 0., 0.))),
        );
    }
    optimize(&mut clip, 1e-3);
    let times: Vec<f32> = clip.track("Head").map(|(t, _)| t).collect();
    assert_eq!(times, [0., 3., 4.]);
}

#[test]
fn local_interpolates_and_holds() {
    let mut s = exported(Quat::IDENTITY);
    s.bones[2].keys = vec![
        (0., Frame::at(Vec3::ZERO)),
        (2., Frame::at(Vec3::new(2., 0., 0.))),
    ];
    assert!(s.local(2, 1.).position.abs_diff_eq(Vec3::X, 1e-6));
    assert!(s
        .local(2, 9.)
        .position
        .abs_diff_eq(Vec3::new(2., 0., 0.), 1e-6));
    assert_eq!(s.local(0, 1.), s.bones[0].rest);
    assert_eq!(s.times(), [0., 2.]);
}
