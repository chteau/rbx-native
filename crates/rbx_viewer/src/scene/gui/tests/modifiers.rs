//! `UICorner`, `UIStroke` and `UIGradient`: what is read off a frame's
//! children and how it resolves against the frame's pixel box.

use rbx_dom::{ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint};

use super::super::plan::{GradientKind, Join, Tile};
use super::*;

fn udim(scale: f32, offset: i32) -> Variant {
    Variant::UDim(UDim { scale, offset })
}

fn color3(r: f32, g: f32, b: f32) -> Variant {
    Variant::Color3(Color3Data { r, g, b })
}

/// A 200×100 frame at the origin, the box every resolution below is against.
fn boxed(dom: &mut WeakDom, gui: Ref) -> Ref {
    frame(dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 200, 0.0, 100))
}

fn corner(dom: &mut WeakDom, parent: Ref, radius: Variant) -> Ref {
    let corner = dom.new_instance("UICorner", "UICorner", Some(parent));
    dom.set_property(corner, "CornerRadius", radius).unwrap();
    corner
}

fn stroke(dom: &mut WeakDom, parent: Ref, thickness: f32) -> Ref {
    let stroke = dom.new_instance("UIStroke", "UIStroke", Some(parent));
    dom.set_property(stroke, "Thickness", Variant::Float32(thickness))
        .unwrap();
    stroke
}

fn gradient(dom: &mut WeakDom, parent: Ref, rotation: f32) -> Ref {
    let gradient = dom.new_instance("UIGradient", "UIGradient", Some(parent));
    dom.set_property(gradient, "Rotation", Variant::Float32(rotation))
        .unwrap();
    gradient
}

fn only(dom: &WeakDom) -> Element {
    let mut elements = resolve(&screens(dom), VIEWPORT);
    assert_eq!(elements.len(), 1);
    elements.remove(0)
}

#[test]
fn a_frame_without_modifiers_has_none() {
    let (mut dom, gui) = screen_gui();
    boxed(&mut dom, gui);

    let element = only(&dom);

    assert_eq!(element.corner_radii, [0.0; 4]);
    assert!(element.strokes.is_empty());
    assert_eq!(element.gradient, None);
}

// The docs make a radius' scale a fraction of the *shorter* side.
#[test]
fn a_corner_scale_is_against_the_shorter_side_and_offset_adds_pixels() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    corner(&mut dom, frame, udim(0.1, 4));

    assert_eq!(only(&dom).corner_radii, [14.0; 4]);
}

// "Rounded rectangles will always be in a pill shape if CornerRadius is set
// to a value that leads to a calculated result greater than half of the
// rectangle's minimum width or height."
#[test]
fn a_radius_is_clamped_to_half_the_shorter_side() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    corner(&mut dom, frame, udim(1.0, 500));

    assert_eq!(only(&dom).corner_radii, [50.0; 4]);
}

#[test]
fn the_individual_radii_win_over_the_shorthand_where_any_is_set() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let corner = corner(&mut dom, frame, udim(0.0, 8));
    dom.set_property(corner, "TopLeftRadius", udim(0.0, 0))
        .unwrap();
    dom.set_property(corner, "TopRightRadius", udim(0.0, 12))
        .unwrap();
    dom.set_property(corner, "BottomRightRadius", udim(0.0, 16))
        .unwrap();
    dom.set_property(corner, "BottomLeftRadius", udim(0.0, 20))
        .unwrap();

    assert_eq!(only(&dom).corner_radii, [0.0, 12.0, 16.0, 20.0]);
}

// A serializer filling in a class default it does not know leaves the four
// at zero beside a real `CornerRadius`; Roblox never writes them disagreeing.
#[test]
fn zeroed_individual_radii_fall_back_to_the_shorthand() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let corner = corner(&mut dom, frame, udim(0.0, 8));
    for name in [
        "TopLeftRadius",
        "TopRightRadius",
        "BottomRightRadius",
        "BottomLeftRadius",
    ] {
        dom.set_property(corner, name, udim(0.0, 0)).unwrap();
    }

    assert_eq!(only(&dom).corner_radii, [8.0; 4]);
}

#[test]
fn a_rounded_box_drops_its_pixel_border() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    dom.set_property(frame, "BorderSizePixel", Variant::Int32(2))
        .unwrap();
    corner(&mut dom, frame, udim(0.0, 8));

    assert_eq!(only(&dom).border, None);
}

#[test]
fn the_first_corner_wins() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    corner(&mut dom, frame, udim(0.0, 8));
    corner(&mut dom, frame, udim(0.0, 30));

    assert_eq!(only(&dom).corner_radii, [8.0; 4]);
}

#[test]
fn a_stroke_defaults_to_an_opaque_black_pixel_outside_the_edge() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    dom.new_instance("UIStroke", "UIStroke", Some(frame));

    let stroke = only(&dom).strokes[0];

    assert_eq!(stroke.color, [0.0; 3]);
    assert_eq!(stroke.alpha, 1.0);
    assert_eq!(stroke.band, [0.0, 1.0]);
    assert_eq!(stroke.join, Join::Round);
    assert!(!stroke.on_text);
}

#[test]
fn a_stroke_reads_its_colour_thickness_transparency_and_join() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let stroke = stroke(&mut dom, frame, 6.0);
    dom.set_property(stroke, "Color", color3(1.0, 1.0, 1.0))
        .unwrap();
    dom.set_property(stroke, "Transparency", Variant::Float32(0.25))
        .unwrap();
    dom.set_property(stroke, "LineJoinMode", Variant::Enum(2))
        .unwrap();

    let stroke = only(&dom).strokes[0];

    assert_eq!(stroke.color, [1.0; 3]);
    assert_eq!(stroke.alpha, 0.75);
    assert_eq!(stroke.band, [0.0, 6.0]);
    assert_eq!(stroke.join, Join::Miter);
}

#[test]
fn a_stroke_band_follows_its_position_sizing_and_offset() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    // `ScaledSize`: 0.1 of the shorter side (100) is 10 px, centred on the
    // edge, then pushed 2 px out by `BorderOffset`.
    let stroke = stroke(&mut dom, frame, 0.1);
    dom.set_property(stroke, "StrokeSizingMode", Variant::Enum(1))
        .unwrap();
    dom.set_property(stroke, "BorderStrokePosition", Variant::Enum(1))
        .unwrap();
    dom.set_property(stroke, "BorderOffset", udim(0.0, 2))
        .unwrap();

    assert_eq!(only(&dom).strokes[0].band, [-3.0, 7.0]);
}

#[test]
fn an_inner_stroke_sits_entirely_inside_the_edge() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let stroke = stroke(&mut dom, frame, 4.0);
    dom.set_property(stroke, "BorderStrokePosition", Variant::Enum(2))
        .unwrap();

    assert_eq!(only(&dom).strokes[0].band, [-4.0, 0.0]);
}

// The docs: `Enabled = false` is a stroke "not rendered"; every enabled
// sibling is, "relative to sibling `UIStroke` instances" by `ZIndex`, lower
// under higher.
#[test]
fn a_disabled_stroke_is_skipped_and_the_enabled_ones_come_in_z_index_order() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let off = stroke(&mut dom, frame, 20.0);
    dom.set_property(off, "Enabled", Variant::Bool(false))
        .unwrap();
    let over = stroke(&mut dom, frame, 3.0);
    dom.set_property(over, "ZIndex", Variant::Int32(2)).unwrap();
    stroke(&mut dom, frame, 9.0);

    let bands: Vec<[f32; 2]> = only(&dom)
        .strokes
        .iter()
        .map(|stroke| stroke.band)
        .collect();
    assert_eq!(bands, [[0.0, 9.0], [0.0, 3.0]]);
}

// On a text class a `Contextual` stroke outlines the glyphs; `Border` forces
// the box. On anything else `Contextual` is the box too.
#[test]
fn a_contextual_stroke_is_on_text_only_for_a_text_class() {
    let (mut dom, gui) = screen_gui();
    let label = dom.new_instance("TextLabel", "TextLabel", Some(gui));
    dom.set_property(label, "Size", udim2(0.0, 200, 0.0, 100))
        .unwrap();
    let contextual = stroke(&mut dom, label, 2.0);
    let plain = boxed(&mut dom, gui);
    stroke(&mut dom, plain, 2.0);

    let elements = resolve(&screens(&dom), VIEWPORT);
    assert!(elements[0].strokes[0].on_text);
    assert!(!elements[1].strokes[0].on_text);

    dom.set_property(contextual, "ApplyStrokeMode", Variant::Enum(1))
        .unwrap();
    let elements = resolve(&screens(&dom), VIEWPORT);
    assert!(!elements[0].strokes[0].on_text);
}

#[test]
fn a_gradient_defaults_to_a_flat_opaque_white_ramp_left_to_right() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    dom.new_instance("UIGradient", "UIGradient", Some(frame));

    let gradient = only(&dom).gradient.unwrap();

    assert_eq!(gradient.color.keypoints.len(), 1);
    assert_eq!(gradient.transparency.keypoints.len(), 1);
    assert_eq!(gradient.origin, [0.0, 0.0]);
    assert_eq!(gradient.kind, GradientKind::Linear);
    assert_eq!(gradient.tile, Tile::Clamp);
    // Rotation 0: `t` moves by one across the 200 px width.
    assert!((gradient.axis[0] - 1.0 / 200.0).abs() < 1e-6);
    assert_eq!(gradient.axis[1], 0.0);
}

#[test]
fn a_gradient_keeps_its_sequences_and_reads_its_enums() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let gradient = gradient(&mut dom, frame, 0.0);
    let keypoint = |time, value| NumberSequenceKeypoint {
        time,
        value,
        envelope: 0.0,
    };
    dom.set_property(
        gradient,
        "Transparency",
        Variant::NumberSequence(NumberSequence {
            keypoints: vec![keypoint(0.0, 0.0), keypoint(1.0, 0.5)],
        }),
    )
    .unwrap();
    dom.set_property(
        gradient,
        "Color",
        Variant::ColorSequence(ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                envelope: 0.0,
            }],
        }),
    )
    .unwrap();
    dom.set_property(gradient, "Type", Variant::Enum(1))
        .unwrap();
    dom.set_property(gradient, "TileMode", Variant::Enum(2))
        .unwrap();

    let gradient = only(&dom).gradient.unwrap();

    assert_eq!(gradient.transparency.keypoints[1].value, 0.5);
    assert_eq!(gradient.color.keypoints[0].color.r, 1.0);
    assert_eq!(gradient.kind, GradientKind::Radial);
    assert_eq!(gradient.tile, Tile::Mirror);
}

// "Clockwise rotation in degrees starting from left to right": a quarter
// turn runs top to bottom, spanning the 100 px height.
#[test]
fn a_gradient_rotated_a_quarter_turn_runs_down_the_height() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    gradient(&mut dom, frame, 90.0);

    let axis = only(&dom).gradient.unwrap().axis;

    assert!(axis[0].abs() < 1e-6);
    assert!((axis[1] - 1.0 / 100.0).abs() < 1e-6);
}

// "The beginning and end control points snap to the edges of the parent":
// at 45 degrees the ramp spans the box's projection onto the diagonal.
#[test]
fn a_diagonal_gradient_spans_the_boxs_projection_onto_its_direction() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    gradient(&mut dom, frame, 45.0);

    let axis = only(&dom).gradient.unwrap().axis;

    let span = (200.0 + 100.0) * std::f32::consts::FRAC_1_SQRT_2;
    let expected = std::f32::consts::FRAC_1_SQRT_2 / span;
    assert!((axis[0] - expected).abs() < 1e-6);
    assert!((axis[1] - expected).abs() < 1e-6);
}

// "(1, 0) shifts the gradient horizontally to the right by a distance equal
// to the parent object's size."
#[test]
fn a_gradient_offset_is_in_units_of_the_boxs_own_size() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let gradient = gradient(&mut dom, frame, 0.0);
    dom.set_property(
        gradient,
        "Offset",
        Variant::Vector2(Vector2Data { x: 0.5, y: -1.0 }),
    )
    .unwrap();

    assert_eq!(only(&dom).gradient.unwrap().origin, [100.0, -100.0]);
}

// `Scale` stretches the ramp: at 2 only its middle half crosses the box.
#[test]
fn a_gradient_scale_stretches_the_ramp_and_zero_reads_as_unset() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let gradient = gradient(&mut dom, frame, 0.0);
    dom.set_property(gradient, "Scale", Variant::Float32(2.0))
        .unwrap();
    assert!((only(&dom).gradient.unwrap().axis[0] - 1.0 / 400.0).abs() < 1e-6);

    dom.set_property(gradient, "Scale", Variant::Float32(0.0))
        .unwrap();
    assert!((only(&dom).gradient.unwrap().axis[0] - 1.0 / 200.0).abs() < 1e-6);
}

// "The radius is defined by the average of the element's width and height
// divided by two, effectively (width+height)/4."
#[test]
fn a_radial_gradient_carries_the_reciprocal_of_that_radius() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let gradient = gradient(&mut dom, frame, 0.0);
    dom.set_property(gradient, "Type", Variant::Enum(1))
        .unwrap();

    let axis = only(&dom).gradient.unwrap().axis;

    assert!((axis[0] - 1.0 / 75.0).abs() < 1e-6);
    assert_eq!(axis[1], 0.0);
}

#[test]
fn a_disabled_gradient_is_ignored() {
    let (mut dom, gui) = screen_gui();
    let frame = boxed(&mut dom, gui);
    let gradient = gradient(&mut dom, frame, 0.0);
    dom.set_property(gradient, "Enabled", Variant::Bool(false))
        .unwrap();

    assert_eq!(only(&dom).gradient, None);
}
