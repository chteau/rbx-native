use super::*;

fn entry(on_top: bool, opaque: bool, order: i32, z: f32) -> Entry {
    Entry {
        draws: Vec::new(),
        on_top,
        opaque,
        order,
        centre: Vec3::new(0.0, 0.0, z),
    }
}

/// At the origin, looking down -Z: all a sort reads of a perspective
/// matrix is its clip w, `-z`.
fn camera() -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::X,
        glam::Vec4::Y,
        glam::Vec4::new(0.0, 0.0, 0.0, -1.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 0.0),
    )
}

// Coplanar but side by side: the one further from the eye is not further
// along the view, so creation order decides and the later one is on top.
#[test]
fn depth_along_the_view_not_distance_from_the_eye_orders_them() {
    let mut near = entry(true, true, 0, -5.0);
    near.centre.x = 0.5;
    let mut off_axis = entry(true, true, 0, -5.0);
    off_axis.centre.x = 4.0;
    let (_, on_top) = draw_order(&[near.clone(), off_axis.clone()], camera());
    assert_eq!(on_top, [0, 1]);
    let (_, on_top) = draw_order(&[off_axis, near], camera());
    assert_eq!(on_top, [0, 1]);
}

// Studio, measured 2026-11-24: opaque depth-tested adornments go first in
// creation order (the depth buffer settles the rest), the blended ones far
// to near after them, ties to the later one.
#[test]
fn depth_tested_adornments_are_opaque_first_then_blended_far_to_near() {
    let entries = [
        entry(false, false, 0, -1.0),
        entry(false, true, 0, -9.0),
        entry(false, false, 0, -5.0),
        entry(false, true, 0, -2.0),
        entry(false, false, 0, -5.0),
    ];
    let (layered, on_top) = draw_order(&entries, camera());
    assert_eq!(layered, [1, 3, 2, 4, 0]);
    assert!(on_top.is_empty());
}

// `ZIndex` first, then far to near, then creation order: the later of two
// crossing boxes at the same depth is drawn over on both sides.
#[test]
fn always_on_top_adornments_sort_by_z_index_then_distance_then_creation() {
    let entries = [
        entry(true, true, 5, -1.0),
        entry(true, true, 1, -1.0),
        entry(true, true, 1, -8.0),
        entry(true, false, 1, -1.0),
        entry(false, true, 9, -1.0),
    ];
    let (layered, on_top) = draw_order(&entries, camera());
    assert_eq!(layered, [4]);
    assert_eq!(on_top, [2, 1, 3, 0]);
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn colours_are_handed_over_srgb_encoded() {
    let got = encoded([1.0, 0.0, crate::scene::srgb_to_linear(0.5)], 1.5);
    assert!(close(got, [1.0, 0.0, 0.5, 1.0]), "{got:?}");
}

// White lands on 127 in Studio for an `AlwaysOnTop` line, 223 (white
// through the tone curve) for a depth-tested one.
#[test]
fn only_an_always_on_top_line_is_drawn_at_half_its_colour() {
    assert!(close(line_color([1.0; 3], 0.5, true), [0.5, 0.5, 0.5, 0.5]));
    assert!(close(
        line_color([1.0; 3], 0.5, false),
        [1.0, 1.0, 1.0, 0.5]
    ));
}
