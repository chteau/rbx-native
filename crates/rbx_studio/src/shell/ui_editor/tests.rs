use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::{is_gui_object, root_of, takes_resolution};

// What decides which root is on the canvas: the nearest the selection
// is in, found from any depth — a part's `SurfaceGui` as much as a
// `ScreenGui` — and whether the toolbar's resolution applies to it.
#[test]
fn the_canvas_screen_is_the_screen_gui_the_selection_sits_in() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let starter = dom.new_instance("StarterGui", "StarterGui", None);
    let hud = dom.new_instance("ScreenGui", "Hud", Some(starter));
    let folder = dom.new_instance("Folder", "Bits", Some(hud));
    let label = dom.new_instance("TextLabel", "Title", Some(folder));
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let sign = dom.new_instance("Part", "Sign", Some(workspace));
    let surface = dom.new_instance("SurfaceGui", "Face", Some(sign));
    let text = dom.new_instance("TextLabel", "Text", Some(surface));

    assert_eq!(root_of(&dom, &database, hud), Some(hud));
    assert_eq!(root_of(&dom, &database, label), Some(hud));
    assert_eq!(root_of(&dom, &database, text), Some(surface));
    assert_eq!(root_of(&dom, &database, sign), None);
    assert!(takes_resolution(&dom, &database, hud));
    assert!(!takes_resolution(&dom, &database, surface), "its own size");

    assert!(is_gui_object(&dom, &database, label));
    assert!(!is_gui_object(&dom, &database, folder));
    assert!(
        !is_gui_object(&dom, &database, hud),
        "a screen has no Position"
    );
}
