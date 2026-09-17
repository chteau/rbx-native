//! Non-`GuiObject` instances inside a GUI tree: a `Folder` (or anything else
//! that is not drawable) is walked through rather than ending the tree, and
//! its contents behave as the container's own children — see
//! [`super::super::plan::elements`].

use super::*;

/// A `Folder` named `name` under `parent`.
fn folder(dom: &mut WeakDom, parent: Ref, name: &str) -> Ref {
    dom.new_instance("Folder", name, Some(parent))
}

#[test]
fn a_frame_under_a_folder_resolves_against_the_screen() {
    let (mut dom, gui) = screen_gui();
    let hud = folder(&mut dom, gui, "Hud");
    frame(
        &mut dom,
        hud,
        udim2(0.5, 0, 0.25, 0),
        udim2(0.25, 0, 0.5, 0),
    );

    // The folder is no box of its own: the frame reads the viewport, exactly
    // as it would parented straight to the `ScreenGui`.
    assert_eq!(
        rects(&dom),
        [Rect {
            x: 400.0,
            y: 150.0,
            width: 200.0,
            height: 300.0,
        }]
    );
}

#[test]
fn nested_folders_are_all_walked_through() {
    let (mut dom, gui) = screen_gui();
    let outer = folder(&mut dom, gui, "Outer");
    let inner = folder(&mut dom, outer, "Inner");
    frame(
        &mut dom,
        inner,
        udim2(0.0, 10, 0.0, 20),
        udim2(0.0, 30, 0.0, 40),
    );

    assert_eq!(
        rects(&dom),
        [Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        }]
    );
}

#[test]
fn a_frame_under_a_folder_takes_the_enclosing_frames_rect_and_clip() {
    let (mut dom, gui) = screen_gui();
    let outer = frame(
        &mut dom,
        gui,
        udim2(0.0, 100, 0.0, 100),
        udim2(0.0, 200, 0.0, 200),
    );
    dom.set_property(outer, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    let group = folder(&mut dom, outer, "Group");
    frame(
        &mut dom,
        group,
        udim2(0.5, 0, 0.0, 0),
        udim2(0.5, 0, 0.5, 0),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    // Half of the *frame*, offset from the frame's corner — the folder is not
    // the nearest `GuiBase2d`, the frame is.
    assert_eq!(
        elements[1].rect,
        Rect {
            x: 200.0,
            y: 100.0,
            width: 100.0,
            height: 100.0,
        }
    );
    assert_eq!(
        elements[1].clip,
        Some(Rect {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
        })
    );
}

#[test]
fn a_folders_children_keep_tree_order_among_its_siblings() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), udim2(0.0, 1, 0.0, 1));
    let group = folder(&mut dom, gui, "Group");
    let middle = frame(&mut dom, group, zero.clone(), udim2(0.0, 2, 0.0, 2));
    frame(&mut dom, gui, zero.clone(), udim2(0.0, 3, 0.0, 3));

    // Paint order is tree order among equal `ZIndex` siblings, and the
    // folder's child sits where the folder itself does.
    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, [1.0, 2.0, 3.0]);

    // And `ZIndex` still sorts it against those siblings rather than only
    // against the folder's other contents.
    dom.set_property(middle, "ZIndex", Variant::Int32(-1))
        .unwrap();
    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, [2.0, 1.0, 3.0]);
}

#[test]
fn an_invisible_frame_hides_a_subtree_hung_off_a_folder() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let hidden = frame(&mut dom, gui, zero.clone(), zero.clone());
    dom.set_property(hidden, "Visible", Variant::Bool(false))
        .unwrap();
    let group = folder(&mut dom, hidden, "Group");
    frame(&mut dom, group, zero.clone(), zero.clone());

    // The walk must not descend *around* the invisible element: `element`
    // answers `None` for it, and the subtree goes with it.
    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}

#[test]
fn a_ui_list_layout_under_a_folder_arranges_nothing() {
    let (mut dom, gui) = screen_gui();
    let group = folder(&mut dom, gui, "Group");
    let list = dom.new_instance("UIListLayout", "UIListLayout", Some(group));
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            group,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 50, 0.0, 50),
        );
    }

    // A `UIComponent` modifies its parent, and a `Folder` is not a
    // `GuiObject` — so nothing stacks and both frames stay at the origin.
    // (Roblox's `Folder` page does document a folder-owned `UILayout`
    // arranging the folder's contents; that is not modelled — see
    // `plan::elements`.)
    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[0.0, 0.0], [0.0, 0.0]]);
}

#[test]
fn a_folders_contents_are_arranged_by_the_containers_own_layout() {
    let (mut dom, gui) = screen_gui();
    let list = dom.new_instance("UIListLayout", "UIListLayout", Some(gui));
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 50, 0.0, 50),
    );
    let group = folder(&mut dom, gui, "Group");
    frame(
        &mut dom,
        group,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 50, 0.0, 50),
    );

    // Stacked one under the other — the default alignment centres the run of
    // 100 px in the 600 px canvas. Deliberately *not* Roblox's documented
    // exemption of folder contents from a sibling `UILayout`; see
    // `plan::elements`.
    let tops: Vec<f32> = rects(&dom).iter().map(|rect| rect.y).collect();
    assert_eq!(tops, [250.0, 300.0]);
}

#[test]
fn a_non_gui_leaf_contributes_nothing() {
    let (mut dom, gui) = screen_gui();
    dom.new_instance("LocalScript", "LocalScript", Some(gui));
    dom.new_instance("StringValue", "StringValue", Some(gui));

    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}
