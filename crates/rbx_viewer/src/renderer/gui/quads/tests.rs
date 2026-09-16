//! Unit tests for [`super`]: scissor conversion, border bands and run merging.

use super::*;
use crate::scene::{GuiElement, Painted};

fn rect(x: f32, y: f32, width: f32, height: f32) -> GuiRect {
    GuiRect {
        x,
        y,
        width,
        height,
    }
}

fn element(rect: GuiRect, clip: Option<GuiRect>) -> GuiElement {
    GuiElement {
        rect,
        clip,
        rotation: 0.0,
        background: [1.0, 0.0, 0.0],
        background_alpha: 1.0,
        border: None,
        image: None,
    }
}

const VIEWPORT: (u32, u32) = (800, 600);

#[test]
fn a_clip_rounds_outwards_so_no_seam_shows_along_its_own_edge() {
    let scissor = scissor(&rect(10.4, 20.6, 100.2, 50.1), VIEWPORT).unwrap();

    assert_eq!(
        scissor,
        Scissor {
            x: 10,
            y: 20,
            width: 101,
            height: 51,
        }
    );
}

#[test]
fn a_clip_running_off_screen_is_clamped_to_the_target() {
    let scissor = scissor(&rect(-50.0, -50.0, 10_000.0, 10_000.0), VIEWPORT).unwrap();

    assert_eq!(
        scissor,
        Scissor {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        }
    );
}

// `wgpu` rejects a zero-sized scissor, and a fully clipped element has nothing
// to show through one anyway.
#[test]
fn a_clip_with_nothing_left_on_screen_has_no_scissor_at_all() {
    assert!(scissor(&rect(900.0, 0.0, 100.0, 100.0), VIEWPORT).is_none());
    assert!(scissor(&rect(0.0, 0.0, 0.0, 0.0), VIEWPORT).is_none());
    assert!(scissor(&rect(-200.0, 0.0, 100.0, 100.0), VIEWPORT).is_none());
}

#[test]
fn an_element_clipped_away_entirely_contributes_no_vertices() {
    let elements = [element(
        rect(0.0, 0.0, 100.0, 100.0),
        Some(rect(900.0, 900.0, 10.0, 10.0)),
    )];

    let (vertices, runs) = build(&elements, &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
    assert!(runs.is_empty());
}

#[test]
fn a_border_covers_each_corner_exactly_once() {
    let bands = outline(&rect(100.0, 100.0, 50.0, 20.0), 2.0);

    // Top and bottom run the full outer width; the sides only span the box.
    assert_eq!(bands[0], rect(98.0, 98.0, 54.0, 2.0));
    assert_eq!(bands[1], rect(98.0, 120.0, 54.0, 2.0));
    assert_eq!(bands[2], rect(98.0, 100.0, 2.0, 20.0));
    assert_eq!(bands[3], rect(150.0, 100.0, 2.0, 20.0));
}

#[test]
fn a_transparent_background_hides_the_border_with_it() {
    let mut hidden = element(rect(0.0, 0.0, 10.0, 10.0), None);
    hidden.background_alpha = 0.0;
    hidden.border = Some((2.0, [0.0, 0.0, 0.0]));

    let (vertices, _) = build(&[hidden], &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
}

#[test]
fn a_background_and_its_border_are_five_quads_in_one_run() {
    let mut outlined = element(rect(0.0, 0.0, 10.0, 10.0), None);
    outlined.border = Some((1.0, [0.0, 0.0, 0.0]));

    let (vertices, runs) = build(&[outlined], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 5 * 6);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].texture, WHITE);
}

#[test]
fn consecutive_elements_sharing_a_texture_and_a_scissor_merge_into_one_run() {
    let elements = [
        element(rect(0.0, 0.0, 10.0, 10.0), None),
        element(rect(20.0, 0.0, 10.0, 10.0), None),
        element(
            rect(40.0, 0.0, 10.0, 10.0),
            Some(rect(0.0, 0.0, 100.0, 100.0)),
        ),
    ];

    let (_, runs) = build(&elements, &HashMap::new(), VIEWPORT);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].range, 0..12);
    assert_eq!(runs[1].range, 12..18);
}

#[test]
fn an_image_whose_asset_never_downloaded_leaves_only_its_background() {
    let mut label = element(rect(0.0, 0.0, 10.0, 10.0), None);
    label.image = Some(Painted {
        asset: AssetRef::Id(7),
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        repeat: [1.0, 1.0],
    });

    let (vertices, runs) = build(&[label], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 6);
    assert_eq!(runs.len(), 1);
}

#[test]
fn a_tiled_image_carries_its_repeat_count_into_the_uvs() {
    let mut label = element(rect(0.0, 0.0, 300.0, 200.0), None);
    label.background_alpha = 0.0;
    label.image = Some(Painted {
        asset: AssetRef::Id(7),
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        repeat: [3.0, 2.0],
    });
    let textures = HashMap::from([(AssetRef::Id(7), 1)]);

    let (vertices, runs) = build(&[label], &textures, VIEWPORT);

    assert_eq!(runs[0].texture, 1);
    let corners: Vec<[f32; 2]> = vertices.iter().map(|vertex| vertex.uv).collect();
    assert!(corners.contains(&[0.0, 0.0]));
    assert!(corners.contains(&[3.0, 2.0]));
}

#[test]
fn a_zero_sized_element_draws_nothing() {
    let elements = [element(rect(10.0, 10.0, 0.0, 50.0), None)];

    let (vertices, _) = build(&elements, &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
}

#[test]
fn zero_rotation_is_the_identity() {
    let spin = Spin::new(0.0, [10.0, 5.0]);

    assert_eq!(spin.apply([0.0, 0.0]), [0.0, 0.0]);
    assert_eq!(spin.apply([12.0, 30.0]), [12.0, 30.0]);
}

#[test]
fn rotation_turns_a_point_clockwise_about_the_pivot() {
    // A square centred on the pivot: turning it 90 degrees clockwise sends
    // the point that was top-left to where top-right used to be — "up" swings
    // to "right" the way a clock's hand does, in this y-down pixel space.
    let spin = Spin::new(90.0, [10.0, 10.0]);
    let top_left = [5.0, 5.0];
    let top_right = [15.0, 5.0];

    let rotated = spin.apply(top_left);

    assert!((rotated[0] - top_right[0]).abs() < 1e-4);
    assert!((rotated[1] - top_right[1]).abs() < 1e-4);
}

#[test]
fn a_rotated_quad_stays_centred_on_the_unrotated_rects_centre() {
    // `GuiObject.Rotation` always turns about the element's own centre and
    // gives no way to move that pivot (not even via `AnchorPoint`), per
    // Roblox's own docs for the property.
    let mut spun = element(rect(0.0, 0.0, 20.0, 10.0), None);
    spun.rotation = 90.0;

    let (vertices, _) = build(&[spun], &HashMap::new(), VIEWPORT);

    let centre_x = vertices.iter().map(|v| v.position[0]).sum::<f32>() / vertices.len() as f32;
    let centre_y = vertices.iter().map(|v| v.position[1]).sum::<f32>() / vertices.len() as f32;
    assert!((centre_x - 10.0).abs() < 1e-3);
    assert!((centre_y - 5.0).abs() < 1e-3);
    // Rotated a quarter turn, the box's own axes swap: it now spans as far
    // vertically as it used to horizontally.
    let min_y = vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    let max_y = vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
    assert!((max_y - min_y - 20.0).abs() < 1e-3);
}
