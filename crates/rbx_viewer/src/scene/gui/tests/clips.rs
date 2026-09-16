//! `ClipsDescendants`: plain scissoring/compounding, its incompatibility with
//! `Rotation`, and the geometric `Rect::intersect` it's built on.

use super::*;

#[test]
fn clips_descendants_scissors_children_to_the_frame_and_compounds() {
    let (mut dom, gui) = screen_gui();
    let outer = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 200),
    );
    dom.set_property(outer, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    // Sticks out to x=300, and clips again 50 px narrower than that.
    let inner = frame(
        &mut dom,
        outer,
        udim2(0.0, 100, 0.0, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    dom.set_property(inner, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    frame(
        &mut dom,
        inner,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    // The clipping frame itself is never clipped by its own flag.
    assert_eq!(elements[0].clip, None);
    assert_eq!(
        elements[1].clip,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        })
    );
    // Both rects intersected: x 100..200, y 0..100.
    assert_eq!(
        elements[2].clip,
        Some(Rect {
            x: 100.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        })
    );
}

#[test]
fn a_non_zero_rotation_makes_the_elements_own_clips_descendants_a_no_op() {
    // Roblox's own docs for `ClipsDescendants` are explicit that — without
    // `StarterGui.ClipsDescendantsSupportsRotation` enabled, which isn't
    // scriptable and so isn't modelled here — it is ignored wherever this
    // element or an ancestor has a non-zero `Rotation`, since a scissor rect
    // can't follow a rotated box.
    let (mut dom, gui) = screen_gui();
    let rotated = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 100),
    );
    dom.set_property(rotated, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    dom.set_property(rotated, "Rotation", Variant::Float32(45.0))
        .unwrap();
    // Sticks out well past its rotated parent's own box.
    frame(
        &mut dom,
        rotated,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 500, 0.0, 500),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].clip, None);
    assert_eq!(elements[1].clip, None);
}

#[test]
fn a_rotated_ancestor_also_disables_a_non_rotated_descendants_clips_descendants() {
    let (mut dom, gui) = screen_gui();
    let rotated = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 100),
    );
    dom.set_property(rotated, "Rotation", Variant::Float32(30.0))
        .unwrap();
    let clipper = frame(
        &mut dom,
        rotated,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 50, 0.0, 50),
    );
    dom.set_property(clipper, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    frame(
        &mut dom,
        clipper,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 500, 0.0, 500),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    // `clipper` itself never rotates, but its rotated ancestor still ignores
    // its `ClipsDescendants` per Roblox's own documented behaviour.
    assert_eq!(elements[2].clip, None);
}

#[test]
fn an_unrotated_clip_still_bounds_a_rotated_descendant() {
    // The incompatibility is about the object that owns `ClipsDescendants`,
    // not about whatever it clips — an unrotated clipping ancestor still
    // scissors a rotated child.
    let (mut dom, gui) = screen_gui();
    let clipper = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 100),
    );
    dom.set_property(clipper, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    let spun = frame(
        &mut dom,
        clipper,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 500, 0.0, 500),
    );
    dom.set_property(spun, "Rotation", Variant::Float32(15.0))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(
        elements[1].clip,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        })
    );
}

#[test]
fn a_clip_that_misses_its_parent_entirely_is_empty_rather_than_negative() {
    let left = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    let right = Rect {
        x: 100.0,
        y: 100.0,
        width: 10.0,
        height: 10.0,
    };

    let overlap = left.intersect(&right);

    assert_eq!(overlap.width, 0.0);
    assert_eq!(overlap.height, 0.0);
}
