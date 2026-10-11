use rbx_animation::{Clip, Direction, Frame, PoseKey, Rig, Style};
use rbx_dom::{Variant, WeakDom};

use super::*;

fn at(x: f32) -> Frame {
    Frame::at(glam::Vec3::new(x, 0., 0.))
}

fn editor() -> AnimEditor {
    let mut e = AnimEditor::default();
    let mut clip = Clip::default();
    clip.set_pose(0.0, "Head", PoseKey::new(at(0.)));
    clip.set_pose(1.0, "Head", PoseKey::new(at(1.)));
    clip.set_pose(1.0, "Torso", PoseKey::new(at(2.)));
    e.load(clip, None);
    e
}

#[test]
fn keying_snaps_to_frames_and_undoes() {
    let mut e = editor();
    e.set_time(0.51);
    assert!((e.time - 0.5).abs() < 1e-6);
    e.key("Head", at(9.));
    assert_eq!(e.clip.track("Head").count(), 3);
    assert!(e.dirty && e.can_undo());
    assert!(e.undo());
    assert_eq!(e.clip.track("Head").count(), 2);
    assert!(e.redo());
    assert_eq!(e.clip.track("Head").count(), 3);
    assert!(!e.redo());
}

#[test]
fn rekeying_keeps_the_easing() {
    let mut e = editor();
    e.select(("Head".into(), 1.0), false);
    e.set_easing(Some(Style::Bounce), Some(Direction::Out));
    e.set_time(1.0);
    e.key("Head", at(5.));
    let key = e.clip.pose(1.0, "Head").unwrap();
    assert_eq!((key.style, key.direction), (Style::Bounce, Direction::Out));
    assert_eq!(key.cframe, at(5.));
}

#[test]
fn selection_extends_toggles_and_takes_whole_keyframes() {
    let mut e = editor();
    e.select(("Head".into(), 0.0), false);
    e.select(("Torso".into(), 1.0), true);
    assert_eq!(e.selected.len(), 2);
    e.select(("Torso".into(), 1.0), true);
    assert_eq!(e.selected.len(), 1);
    e.select_keyframe(1.0, false);
    assert_eq!(e.selected.len(), 2);
    assert!(e.is_selected("Torso", 1.0));
    assert_eq!(e.easing(), Some((Style::Linear, Direction::In)));
}

#[test]
fn dragging_moves_keys_as_one_undo_step_and_clamps_at_zero() {
    let mut e = editor();
    e.select_keyframe(1.0, false);
    e.begin_drag();
    e.drag_by(0.26);
    e.drag_by(0.51);
    assert!(e.clip.pose(1.5, "Torso").is_some());
    e.drag_by(-5.0);
    assert!(e.clip.pose(0.0, "Torso").is_some(), "clamped to the start");
    e.end_drag();
    assert!(!e.dragging());
    assert!(e.undo());
    assert!(e.clip.pose(1.0, "Torso").is_some());
}

#[test]
fn delete_copy_paste() {
    let mut e = editor();
    e.select_keyframe(1.0, false);
    assert_eq!(e.copy(), 2);
    e.set_time(2.0);
    assert!(e.paste());
    assert!(e.clip.pose(2.0, "Torso").is_some());
    assert!(e.delete_selected());
    assert!(e.clip.pose(2.0, "Torso").is_none());
    assert!(!e.delete_selected());
}

#[test]
fn playback_wraps_when_looped_and_stops_otherwise() {
    let mut e = editor();
    e.toggle_play();
    assert!(e.tick(0.6));
    assert!(!e.tick(0.6));
    assert_eq!(e.time, 1.0);
    e.clip.looped = true;
    e.toggle_play();
    assert_eq!(e.time, 0.0, "replay from the start");
    e.tick(1.25);
    assert!((e.time - 0.25).abs() < 1e-5);
    e.step_frames(3);
    assert!(!e.playing);
    assert_eq!(e.frame(), 11);
    e.jump_key(true);
    assert_eq!(e.time, 1.0);
    e.jump_key(false);
    assert_eq!(e.time, 0.0);
}

#[test]
fn markers_and_key_all() {
    let mut e = editor();
    e.set_time(0.5);
    e.add_marker("Footstep", "Left");
    assert_eq!(e.clip.keyframes[1].markers[0].name, "Footstep");
    assert!(e.remove_markers_here());
    assert_eq!(e.clip.keyframes.len(), 2, "an emptied keyframe goes");
    e.key_all(&[("Head".into(), at(1.)), ("Torso".into(), at(1.))]);
    assert_eq!(e.selected.len(), 2);
}

#[test]
fn rows_follow_the_rig_and_flag_strangers() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "R", None);
    let part = |dom: &mut WeakDom, name: &str| dom.new_instance("Part", name, Some(model));
    let root = part(&mut dom, "HumanoidRootPart");
    let torso = part(&mut dom, "Torso");
    let head = part(&mut dom, "Head");
    let arm = part(&mut dom, "Arm");
    for (p0, p1) in [(root, torso), (torso, head), (torso, arm)] {
        let m = dom.new_instance("Motor6D", "J", Some(p1));
        dom.set_property(m, "Part0", Variant::Ref(p0)).unwrap();
        dom.set_property(m, "Part1", Variant::Ref(p1)).unwrap();
    }
    let rig = Rig::read(&dom, model);
    let mut clip = Clip::default();
    clip.set_pose(0., "Tail", PoseKey::new(at(0.)));
    let list = rows(Some(&rig), &clip);
    let names: Vec<(&str, usize, bool)> = list
        .iter()
        .map(|r| (r.name.as_str(), r.depth, r.in_rig))
        .collect();
    assert_eq!(
        names,
        [
            ("Torso", 0, true),
            ("Head", 1, true),
            ("Arm", 1, true),
            ("Tail", 0, false)
        ]
    );
    assert_eq!(rows(None, &clip)[0].in_rig, true);
}
