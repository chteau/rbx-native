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

#[test]
fn a_square_corner_s_handle_sits_clear_of_the_corner_and_a_round_one_at_its_radius() {
    use super::gesture::{radius_at, radius_handles};
    use crate::ui_canvas::Rect;
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    let square = radius_handles(&rect, 0.0, [0.0; 4], 1.0);
    assert_eq!(square[0], ([-1, -1], [12.0, 12.0]));
    let round = radius_handles(&rect, 0.0, [30.0; 4], 1.0);
    assert_eq!(round[2], ([1, 1], [170.0, 70.0]));
    // Dragged back onto where it stands, the handle reads its radius.
    assert_eq!(radius_at(&rect, 0.0, [1, 1], [170.0, 70.0]), 30.0);
    // Never past a pill's round end, nor out past the corner.
    assert_eq!(radius_at(&rect, 0.0, [-1, -1], [150.0, 90.0]), 50.0);
    assert_eq!(radius_at(&rect, 0.0, [-1, -1], [-20.0, -20.0]), 0.0);
    // Too small on screen for them: none at all.
    assert!(radius_handles(&rect, 0.0, [0.0; 4], 0.2).is_empty());
}
