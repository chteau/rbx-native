use glam::Vec3;
use rbx_animation::curve::{Channel, Interpolation};
use rbx_animation::facs::FaceKey;
use rbx_animation::{Clip, Direction, Frame, PoseKey, Style};

use super::super::AnimEditor;
use super::CurveKeyId;

fn at(x: f32) -> Frame {
    Frame::at(Vec3::new(x, 0., 0.))
}

fn editor() -> AnimEditor {
    let mut e = AnimEditor::default();
    let mut clip = Clip::default();
    clip.set_pose(0.0, "Head", PoseKey::new(at(0.)));
    clip.set_pose(1.0, "Head", PoseKey::new(at(1.)));
    e.load(clip, None);
    e
}

fn id(time: f32) -> CurveKeyId {
    CurveKeyId {
        joint: "Head".into(),
        channel: Channel::PositionX,
        time,
    }
}

fn head_x(e: &AnimEditor, time: f32) -> f32 {
    e.sample_at(time)["Head"].position.x
}

#[test]
fn converting_to_curves_keeps_the_motion_and_undoes() {
    let mut e = editor();
    assert!(e.convert_to_curves());
    assert!(!e.convert_to_curves(), "already curves");
    let curves = e.curves.as_ref().unwrap();
    assert_eq!(
        curves.joints["Head"].curve(Channel::PositionX).keys.len(),
        2
    );
    assert!((head_x(&e, 0.5) - 0.5).abs() < 1e-4);
    assert!(e.undo());
    assert!(e.curves.is_none());
    assert!(e.redo());
    assert!(e.curves.is_some());
    assert!(e.back_to_keyframes());
    assert!(e.curves.is_none());
    assert!((head_x(&e, 0.5) - 0.5).abs() < 1e-4);
}

#[test]
fn editing_a_key_rebakes_the_clip() {
    let mut e = editor();
    e.convert_to_curves();
    e.select_curve_key(id(1.0), false);
    assert!(e.is_curve_selected(&id(1.0)));
    e.set_curve_value(3.0);
    assert!((head_x(&e, 1.0) - 3.0).abs() < 1e-5);
    // The baked clip carries the same motion for the dope sheet and Save.
    let baked = e.clip.pose(1.0, "Head").unwrap().cframe;
    assert!((baked.position.x - 3.0).abs() < 1e-5);
    assert!(e.undo());
    assert!((head_x(&e, 1.0) - 1.0).abs() < 1e-5);
}

#[test]
fn cubic_keys_bake_into_frames_and_tangents_shape_them() {
    let mut e = editor();
    e.convert_to_curves();
    e.select_curve_key(id(0.0), false);
    e.set_curve_interpolation(Interpolation::Cubic);
    assert_eq!(e.curve_interpolation(), Some(Interpolation::Cubic));
    e.set_curve_tangents(Some(0.), Some(0.));
    // A flat start eases in: behind the straight line early on.
    assert!(head_x(&e, 0.25) < 0.25);
    assert!(
        e.clip.track("Head").count() > 2,
        "the cubic segment is baked"
    );
    e.auto_curve_tangents();
    let key = e.curve_key(&id(0.0)).unwrap();
    assert_eq!((key.left_tangent, key.right_tangent), (None, None));
}

#[test]
fn inserting_and_deleting_keys() {
    let mut e = editor();
    e.convert_to_curves();
    e.set_time(0.5);
    e.insert_curve_key("Head", Channel::PositionX);
    let key = e.curve_key(&id(0.5)).unwrap();
    assert!(
        (key.value - 0.5).abs() < 1e-5,
        "where the curve already was"
    );
    assert_eq!(e.curve_selected, vec![id(0.5)]);
    assert!(e.delete_curve_keys());
    assert!(e.curve_key(&id(0.5)).is_none());
    assert!(!e.delete_curve_keys(), "nothing selected now");
}

#[test]
fn dragging_moves_time_and_value_as_one_undo() {
    let mut e = editor();
    e.convert_to_curves();
    e.select_curve_key(id(1.0), false);
    e.begin_curve_drag();
    e.curve_drag_by(0.2, 0.5, 0.0);
    e.curve_drag_by(0.51, 1.0, 0.0);
    e.end_curve_drag();
    let moved = CurveKeyId {
        time: 1.5,
        ..id(0.)
    };
    assert_eq!(e.curve_selected, vec![moved.clone()]);
    assert!((e.curve_key(&moved).unwrap().value - 2.0).abs() < 1e-5);
    assert!(e.undo());
    assert!(e.curve_key(&id(1.0)).is_some());
    assert!(e.curve_key(&moved).is_none());
}

#[test]
fn generate_curve_bakes_a_bounce_between_the_selected_keys() {
    let mut e = editor();
    e.convert_to_curves();
    e.select_curve_key(id(0.0), false);
    assert!(e.generate_curve(Style::Bounce, Direction::Out));
    let keys = &e.curves.as_ref().unwrap().joints["Head"]
        .curve(Channel::PositionX)
        .keys;
    assert!(keys.len() > 10, "baked at the editor's 30 fps");
    assert!((keys.last().unwrap().value - 1.0).abs() < 1e-5);
}

#[test]
fn keying_from_the_viewport_writes_curve_keys() {
    let mut e = editor();
    e.convert_to_curves();
    e.set_time(0.5);
    e.key(
        "Head",
        Frame::new(Vec3::new(4., 0., 0.), glam::Quat::from_rotation_y(0.5)),
    );
    let curves = &e.curves.as_ref().unwrap().joints["Head"];
    assert_eq!(curves.curve(Channel::PositionX).keys.len(), 3);
    let turn = curves.curve(Channel::RotationY).eval(0.5).unwrap();
    assert!((turn - 0.5).abs() < 1e-4);
    assert!((head_x(&e, 0.5) - 4.0).abs() < 1e-5);
}

#[test]
fn settings_and_face_keys_survive_with_curves() {
    let mut e = editor();
    e.convert_to_curves();
    e.edit(|c| c.looped = true);
    assert!(e.curves.as_ref().unwrap().looped);
    e.set_time(0.5);
    e.key_face("JawDrop", 0.7, false);
    e.select_curve_key(id(1.0), false);
    e.set_curve_value(2.0);
    let face = e.clip.face_track("JawDrop").next().map(|(t, k)| (t, *k));
    assert_eq!(face, Some((0.5, FaceKey::new(0.7))));
}

#[test]
fn curve_animation_round_trips_through_a_written_place() {
    use rbx_animation::curve::file;
    let mut e = editor();
    assert!(e.curve_animation().is_none());
    e.convert_to_curves();
    e.set_time(0.5);
    e.key_face("JawDrop", 0.7, false);
    e.clip.keyframe_mut(0.5).name = "Wave".into();
    let a = e.curve_animation().unwrap();
    assert_eq!(a.faces["JawDrop"].keys.len(), 1);
    assert_eq!(a.keyframe_names, vec![(0.5, "Wave".to_owned())]);

    let mut dom = rbx_dom::WeakDom::new();
    let r = file::write(&mut dom, None, &a, None);
    let back = file::read(&dom, r).unwrap();
    let mut other = AnimEditor::default();
    other.load_curves(back, None);
    assert!(other.curves.is_some());
    assert!(!other.clip.keyframes.is_empty());
    assert!(!other.dirty);
    assert_eq!(other.curve_animation(), Some(a));
}

#[test]
fn a_tangent_drag_and_a_retime_are_each_one_undo() {
    let mut e = editor();
    e.convert_to_curves();
    e.select_curve_key(id(0.0), false);
    e.set_curve_interpolation(Interpolation::Cubic);
    e.begin_curve_drag();
    e.drag_tangent(false, 0.2);
    e.drag_tangent(false, 3.0);
    e.end_curve_drag();
    let key = e.curve_key(&id(0.0)).unwrap();
    assert_eq!(key.right_tangent, Some(3.0));
    assert!(e.undo());
    assert_eq!(e.curve_key(&id(0.0)).unwrap().right_tangent, None);

    e.select_curve_key(id(1.0), false);
    e.set_curve_time(1.49);
    assert!(e.curve_key(&id(1.5)).is_some());
    assert_eq!(e.curve_selected, vec![id(1.5)]);
}

#[test]
fn a_cubic_face_curve_survives_save_and_load_unchanged() {
    use rbx_animation::curve::file;
    let mut e = editor();
    e.convert_to_curves();
    for (time, value) in [(0.0, 0.0), (1.0, 1.0)] {
        let mut face = FaceKey::new(value);
        face.style = Style::Cubic;
        e.clip.set_face(time, "JawDrop", face);
    }
    let a = e.curve_animation().unwrap();
    assert_eq!(a.faces["JawDrop"].keys.len(), 2);
    let mut dom = rbx_dom::WeakDom::new();
    let r = file::write(&mut dom, None, &a, None);
    let mut other = AnimEditor::default();
    other.load_curves(file::read(&dom, r).unwrap(), Some(r));
    assert_eq!(other.curve_animation(), Some(a));
}
