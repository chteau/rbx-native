//! `CanvasGroup`: its forced clip, and the subtree run it hands the renderer
//! to flatten — only under `ZIndexBehavior.Sibling`.

use super::super::plan::GroupTint as Group;
use super::super::Grouped;
use super::*;
use crate::scene::srgb_to_linear;

#[test]
fn a_canvas_group_always_clips_and_hands_the_renderer_its_subtree() {
    let (mut dom, gui) = screen_gui();
    let group = dom.new_instance("CanvasGroup", "Group", Some(gui));
    dom.set_property(group, "Size", udim2(0.0, 100, 0.0, 100))
        .unwrap();
    dom.set_property(group, "ClipsDescendants", Variant::Bool(false))
        .unwrap();
    dom.set_property(group, "GroupTransparency", Variant::Float32(0.5))
        .unwrap();
    dom.set_property(
        group,
        "GroupColor3",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.5,
            b: 0.0,
        }),
    )
    .unwrap();
    let child = frame(
        &mut dom,
        group,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 150, 0.0, 20),
    );
    frame(
        &mut dom,
        child,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(
        elements[0].group,
        Some(Grouped {
            tint: Group {
                color: [1.0, srgb_to_linear(0.5), 0.0],
                alpha: 0.5,
            },
            descendants: 2,
            texture: None,
        })
    );
    assert_eq!(
        elements[1].clip,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        })
    );
    assert_eq!(elements[2].group, None);
}

#[test]
fn a_default_canvas_group_is_still_marked_but_tints_nothing() {
    let (mut dom, gui) = screen_gui();
    let group = dom.new_instance("CanvasGroup", "Group", Some(gui));
    dom.set_property(group, "Size", udim2(0.0, 100, 0.0, 100))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    let grouped = elements[0].group.expect("a CanvasGroup is marked");
    assert!(grouped.tint.is_default());
    assert_eq!(grouped.descendants, 0);
}

// "Descendants of `CanvasGroup` will be rendered as a flattened texture only
// when the ancestor `LayerCollector` has its `ZIndexBehavior` set to
// `Sibling`."
#[test]
fn a_canvas_group_under_global_z_index_is_never_flattened() {
    let (mut dom, gui) = screen_gui();
    dom.set_property(gui, "ZIndexBehavior", Variant::Enum(0))
        .unwrap();
    let group = dom.new_instance("CanvasGroup", "Group", Some(gui));
    dom.set_property(group, "Size", udim2(0.0, 100, 0.0, 100))
        .unwrap();
    dom.set_property(group, "GroupTransparency", Variant::Float32(0.5))
        .unwrap();
    frame(
        &mut dom,
        group,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].group, None);
}
