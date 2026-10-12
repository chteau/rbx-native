use glam::Vec3;

use super::*;

fn key(x: f32) -> PoseKey {
    PoseKey::new(Frame::at(Vec3::new(x, 0., 0.)))
}

#[test]
fn keyframes_stay_sorted_and_merge_by_time() {
    let mut clip = Clip::default();
    clip.set_pose(1.0, "Head", key(1.));
    clip.set_pose(0.5, "Head", key(0.5));
    clip.set_pose(1.00001, "Torso", key(2.));
    let times: Vec<f32> = clip.keyframes.iter().map(|k| k.time).collect();
    assert_eq!(times, [0.5, 1.0]);
    assert_eq!(clip.keyframes[1].poses.len(), 2);
    assert_eq!(clip.length(), 1.0);
    assert_eq!(clip.joints(), ["Head", "Torso"]);
}

#[test]
fn track_lists_only_that_joints_keys() {
    let mut clip = Clip::default();
    clip.set_pose(0., "Head", key(0.));
    clip.set_pose(0.5, "Torso", key(9.));
    clip.set_pose(1., "Head", key(1.));
    let times: Vec<f32> = clip.track("Head").map(|(t, _)| t).collect();
    assert_eq!(times, [0., 1.]);
}

#[test]
fn removing_the_last_pose_removes_the_keyframe() {
    let mut clip = Clip::default();
    clip.set_pose(0.5, "Head", key(0.));
    assert!(clip.remove_pose(0.5, "Head").is_some());
    assert!(clip.keyframes.is_empty());

    clip.set_pose(0.5, "Head", key(0.));
    clip.keyframe_mut(0.5).markers.push(Marker {
        name: "Step".into(),
        value: String::new(),
    });
    clip.remove_pose(0.5, "Head");
    assert_eq!(clip.keyframes.len(), 1, "a marker keeps its keyframe");
}

#[test]
fn moving_keys_merges_into_existing_keyframes() {
    let mut clip = Clip::default();
    clip.set_pose(0., "Head", key(0.));
    clip.set_pose(1., "Torso", key(1.));
    assert!(clip.move_pose(0., 1., "Head"));
    assert_eq!(clip.keyframes.len(), 1);
    assert_eq!(clip.keyframes[0].poses.len(), 2);
    assert!(!clip.move_pose(3., 4., "Head"));

    assert!(clip.move_keyframe(1., 2.));
    assert_eq!(clip.length(), 2.);
    clip.scale_time(0.5);
    assert_eq!(clip.length(), 1.);
}

#[test]
fn priority_values_match_the_enum() {
    for p in Priority::ALL {
        assert_eq!(Priority::from_value(p.value()), p);
    }
    assert_eq!(Priority::Core.value(), 1000);
    assert_eq!(Priority::from_value(77), Priority::Core);
}
