use std::path::Path;

use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{is_video, place};

fn place_with_gui() -> (WeakDom, rbx_dom::Ref, rbx_dom::Ref, rbx_dom::Ref) {
    let mut dom = WeakDom::new();
    let starter = dom.new_instance("StarterGui", "StarterGui", None);
    let screen = dom.new_instance("ScreenGui", "Hud", Some(starter));
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    (dom, starter, screen, part)
}

fn video_of(dom: &WeakDom, frame: rbx_dom::Ref) -> String {
    let properties = dom.get(frame).unwrap().properties();
    match properties
        .get("Video")
        .or_else(|| properties.get("VideoContent"))
    {
        Some(Variant::String(text)) => text.clone(),
        Some(Variant::Content(rbx_dom::Content::Uri(uri))) => uri.clone(),
        other => panic!("no video: {other:?}"),
    }
}

#[test]
fn a_video_lands_in_the_selected_gui() {
    let (mut dom, _, screen, _) = place_with_gui();
    let database = ReflectionDatabase::embedded();
    let frame = place(&mut dom, &database, Some(screen), "Intro", 42).unwrap();
    assert_eq!(dom.parent(frame), Some(screen));
    assert_eq!(dom.get(frame).unwrap().class(), "VideoFrame");
    assert_eq!(dom.get(frame).unwrap().name(), "Intro");
    assert_eq!(video_of(&dom, frame), "rbxassetid://42");
}

#[test]
fn without_a_gui_selected_it_gets_a_screen_gui_of_its_own() {
    let (mut dom, starter, _, part) = place_with_gui();
    let database = ReflectionDatabase::embedded();
    let frame = place(&mut dom, &database, Some(part), "Intro", 7).unwrap();
    let holder = dom.parent(frame).unwrap();
    assert_eq!(dom.get(holder).unwrap().class(), "ScreenGui");
    assert_eq!(dom.get(holder).unwrap().name(), "Intro");
    assert_eq!(dom.parent(holder), Some(starter));
}

#[test]
fn a_place_without_starter_gui_says_so() {
    let mut dom = WeakDom::new();
    let database = ReflectionDatabase::embedded();
    assert!(place(&mut dom, &database, None, "Intro", 1)
        .unwrap_err()
        .contains("StarterGui"));
}

#[test]
fn videos_are_told_by_extension() {
    assert!(is_video(Path::new("clip.mp4")));
    assert!(is_video(Path::new("clip.MOV")));
    assert!(!is_video(Path::new("clip.png")));
}
