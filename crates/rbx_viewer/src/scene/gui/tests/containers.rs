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

/// A vertical `UIListLayout` under `parent`, at Roblox's default (centred)
/// alignment.
fn list_layout(dom: &mut WeakDom, parent: Ref) -> Ref {
    let list = dom.new_instance("UIListLayout", "UIListLayout", Some(parent));
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    list
}

#[test]
fn a_ui_list_layout_under_a_folder_arranges_the_folders_contents() {
    let (mut dom, gui) = screen_gui();
    let group = folder(&mut dom, gui, "Group");
    list_layout(&mut dom, group);
    for _ in 0..2 {
        frame(
            &mut dom,
            group,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 50, 0.0, 50),
        );
    }

    // "Each `Folder` in your UI hierarchy can define its own `UILayout`": the
    // two frames stack, centred in the screen — the box the folder's layout
    // works in is the container's, the folder having none of its own.
    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[375.0, 250.0], [375.0, 300.0]]);
}

#[test]
fn a_folders_contents_are_exempt_from_the_containers_layout() {
    let (mut dom, gui) = screen_gui();
    list_layout(&mut dom, gui);
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

    // "`Folder` contents are exempt from the effects of a `UILayout`
    // sibling": the screen's own child is the only item the list has, and so
    // is centred alone; the folder's child keeps its own `Position` and is
    // not stacked under it.
    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[375.0, 275.0], [0.0, 0.0]]);
}

#[test]
fn a_folder_inside_a_folder_is_a_layout_scope_inside_a_layout_scope() {
    let (mut dom, gui) = screen_gui();
    let outer = folder(&mut dom, gui, "Outer");
    list_layout(&mut dom, outer);
    frame(
        &mut dom,
        outer,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 50, 0.0, 50),
    );
    let inner = folder(&mut dom, outer, "Inner");
    frame(
        &mut dom,
        inner,
        udim2(0.0, 0, 0.0, 100),
        udim2(0.0, 50, 0.0, 50),
    );

    // The inner folder's content is exempt from the outer folder's list the
    // same way it would be from a frame's, and resolves against the same
    // container box.
    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[375.0, 275.0], [0.0, 100.0]]);
}

#[test]
fn a_folders_contents_do_not_grow_an_automatic_size_container() {
    let (mut dom, gui) = screen_gui();
    let box_ = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    // `AutomaticSize.XY`.
    dom.set_property(box_, "AutomaticSize", Variant::Enum(3))
        .unwrap();
    let group = folder(&mut dom, box_, "Group");
    frame(
        &mut dom,
        group,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 200),
    );

    // `AutomaticSize` is documented as fitting "child contents", and a
    // folder's contents are not children of the box — nor are they part of
    // the run a layout would report as `AbsoluteContentSize`, which is "how
    // much space the elements of the grid are taking up". So the box keeps
    // its own 10 x 10 and the content simply overflows it, the way an
    // absolutely positioned child of a shrink-wrapped box does.
    assert_eq!(rects(&dom)[0].size(), [10.0, 10.0]);
}

#[test]
fn a_non_gui_leaf_contributes_nothing() {
    let (mut dom, gui) = screen_gui();
    dom.new_instance("LocalScript", "LocalScript", Some(gui));
    dom.new_instance("StringValue", "StringValue", Some(gui));

    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}
